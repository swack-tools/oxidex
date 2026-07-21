import io
import tempfile
import unittest
from pathlib import Path

from watch_parallel_fix import (
    GREEN,
    RED,
    YELLOW,
    count_tags_found,
    discover_formats,
    discover_workers,
    main,
    parse_round_and_tag,
    parse_status,
    render,
    render_workers,
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


class ParseRoundAndTagTests(unittest.TestCase):
    def test_missing_file_returns_none_none(self):
        round_num, tag = parse_round_and_tag(Path("/nonexistent/worker-1.log"))
        self.assertIsNone(round_num)
        self.assertIsNone(tag)

    def test_extracts_most_recent_round_and_tag(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "worker-1.log"
            path.write_text(
                "[2026-07-20T19:00:00] round 1: attempting JPEG:EXIF:LensModel\n"
                "[2026-07-20T19:00:05] [JPEG:EXIF:LensModel] build failed: no diff\n"
                "[2026-07-20T19:01:00] round 2: attempting JPEG:APP12:CAM1\n"
            )
            round_num, tag = parse_round_and_tag(path)
            self.assertEqual(round_num, 2)
            self.assertEqual(tag, "JPEG:APP12:CAM1")

    def test_no_round_line_yet_returns_none_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "worker-1.log"
            path.write_text("   Compiling oxidex v1.2.1\n")
            round_num, tag = parse_round_and_tag(path)
            self.assertIsNone(round_num)
            self.assertIsNone(tag)


class DiscoverWorkersTests(unittest.TestCase):
    def test_lists_worker_ids_sorted_numerically(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-2.log").write_text("")
            (tmp / "worker-10.log").write_text("")
            (tmp / "worker-1.log").write_text("")
            (tmp / "not-a-worker.log").write_text("")
            # Numeric sort, not lexicographic (10 must not sort before 2).
            self.assertEqual(discover_workers(tmp), [1, 2, 10])

    def test_worker_logs_excluded_from_discover_formats(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text("")
            (tmp / "NEF.log").write_text("")
            self.assertEqual(discover_formats(tmp), ["NEF"])


class CountTagsFoundTests(unittest.TestCase):
    def test_missing_file_is_zero(self):
        self.assertEqual(count_tags_found(Path("/nonexistent/tags-found.log")), 0)

    def test_counts_non_blank_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "tags-found.log"
            path.write_text(
                "2026-07-20T19:00:00 worker=1 tag=JPEG:EXIF:LensModel gaps_closed=1\n"
                "2026-07-20T19:05:00 worker=3 tag=JPEG:APP12:CAM1 gaps_closed=1\n"
                "\n"
            )
            self.assertEqual(count_tags_found(path), 2)


class RenderWorkersTests(unittest.TestCase):
    def test_includes_worker_round_tag_and_aggregate_count(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text(
                "round 3: attempting JPEG:EXIF:LensModel\n[JPEG:EXIF:LensModel] gaps 5 -> 2\n"
            )
            tags_found_log = tmp / "tags-found.log"
            tags_found_log.write_text("2026-07-20T19:00:00 worker=2 tag=X gaps_closed=1\n")

            output = render_workers(tmp, [1], tags_found_log)
            self.assertIn("worker-1", output)
            self.assertIn("round 3", output)
            self.assertIn("JPEG:EXIF:LensModel", output)
            self.assertIn("tags found so far", output)
            self.assertIn("1", output)  # the aggregate count

    def test_worker_with_no_round_yet_shows_placeholder(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text("   Compiling oxidex v1.2.1\n")
            output = render_workers(tmp, [1], tmp / "tags-found.log")
            self.assertIn("round -", output)
            self.assertIn("(none yet)", output)


class MainLoopWorkerModeTests(unittest.TestCase):
    def test_auto_detects_worker_mode_and_shows_aggregate_count(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            sleeps = []

            def fake_sleep(interval):
                sleeps.append(interval)
                if len(sleeps) == 1:
                    (tmp / "worker-1.log").write_text("round 1: attempting JPEG:EXIF:LensModel\n")
                    (tmp / "tags-found.log").write_text("x worker=1 tag=Y gaps_closed=1\n")
                elif len(sleeps) == 2:
                    raise KeyboardInterrupt

            out = io.StringIO()
            exit_code = main(
                ["--log-dir", str(tmp), "--tags-found-log", str(tmp / "tags-found.log"), "--interval", "0.1"],
                sleep_fn=fake_sleep, stdout=out,
            )

            self.assertEqual(exit_code, 0)
            self.assertIn("worker-1", out.getvalue())
            self.assertIn("tags found so far", out.getvalue())


if __name__ == "__main__":
    unittest.main()
