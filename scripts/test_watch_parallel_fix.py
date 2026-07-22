import io
import json
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

from watch_parallel_fix import (
    BRIGHT_GREEN,
    RED,
    YELLOW,
    bar_color,
    blacklist_stats,
    discover_format_progress,
    discover_formats,
    discover_worker_ids,
    discover_workers,
    entries_for_worker,
    find_active_log_dir,
    format_relative,
    found_stats,
    load_tag_state,
    load_worker_model_config,
    main,
    parse_current_round_start,
    parse_current_tag_progress,
    parse_manifest_log,
    parse_tags_found_log,
    parse_timestamp,
    parse_worker_log_status,
    parse_wrapper_log,
    render_dashboard,
    render_format_progress,
    render_progress_bar,
    request_stats,
    tag_iteration,
    worker_log_path,
    worker_worktree_name,
)


class ParseWorkerLogStatusTests(unittest.TestCase):
    def _write(self, tmpdir, text):
        path = Path(tmpdir) / "worker-1.log"
        path.write_text(text)
        return path

    def test_missing_file_is_waiting(self):
        label, color, detail = parse_worker_log_status(Path("/nonexistent/worker-1.log"))
        self.assertEqual(label, "waiting")

    def test_traceback_anywhere_in_log_is_crashed(self):
        # The exact bug that prompted this: a crashed worker's raw
        # exception text used to show up under a generic "busy" label,
        # indistinguishable from ordinary progress -- confirmed to read
        # as an unexplained error on the previous dashboard.
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "Updating crates.io index\n"
                "Traceback (most recent call last):\n"
                '  File "model_fix_loop.py", line 1, in <module>\n'
                "    sys.exit(main())\n"
                "subprocess.CalledProcessError: Command '['cargo', 'build']' returned non-zero exit status 101.\n",
            )
            label, color, detail = parse_worker_log_status(path)
            self.assertEqual(label, "crashed")
            self.assertIn("CalledProcessError", detail)

    def test_blacklisted_line_is_its_own_state(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "round 1: attempting JPEG:APP0:OcadRevision\n"
                "[JPEG:APP0:OcadRevision] blacklisted after 10 failed attempts\n",
            )
            label, color, detail = parse_worker_log_status(path)
            self.assertEqual(label, "blacklisted")
            self.assertEqual(color, RED)

    def test_tag_fixed_line_is_fixed(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "round 1: attempting JPEG:APP12:CAM4\n"
                "[JPEG:APP12:CAM4] FIXED\n",
            )
            label, color, detail = parse_worker_log_status(path)
            self.assertEqual(label, "fixed")
            self.assertEqual(color, BRIGHT_GREEN)

    def test_failed_attempt_not_yet_blacklisted_is_retrying(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "round 1: attempting JPEG:APP12:ExposureTime\n"
                "[JPEG:APP12:ExposureTime] failed attempt 3/10\n",
            )
            label, color, detail = parse_worker_log_status(path)
            self.assertEqual(label, "retrying")
            self.assertEqual(color, YELLOW)

    def test_model_call_retry_is_retrying(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "round 1: attempting JPEG:APP12:ExposureTime\n"
                "model call retry 2/1000 after RuntimeError('empty reply'), waiting 4s\n",
            )
            label, color, detail = parse_worker_log_status(path)
            self.assertEqual(label, "retrying")

    def test_stopped_summary_wins_over_everything_earlier(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "round 1: attempting JPEG:APP12:CAM4\n"
                "[JPEG:APP12:CAM4] FIXED\n"
                "stopped after 1 rounds\n  fixed:   1 tags\n",
            )
            label, color, detail = parse_worker_log_status(path)
            self.assertEqual(label, "done")

    def test_empty_file_is_waiting(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "")
            label, color, detail = parse_worker_log_status(path)
            self.assertEqual(label, "waiting")


class ParseCurrentTagProgressTests(unittest.TestCase):
    def test_missing_file_returns_all_none(self):
        round_num, tag, launched_at = parse_current_tag_progress(Path("/nonexistent/worker-1.log"))
        self.assertIsNone(round_num)
        self.assertIsNone(tag)
        self.assertIsNone(launched_at)

    def test_single_attempt_launched_at_is_that_lines_timestamp(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "worker-1.log"
            path.write_text("[2026-07-20T19:00:00] round 1: attempting JPEG:APP12:CAM1\n")
            round_num, tag, launched_at = parse_current_tag_progress(path)
            self.assertEqual(round_num, 1)
            self.assertEqual(tag, "JPEG:APP12:CAM1")
            self.assertEqual(launched_at, parse_timestamp("2026-07-20T19:00:00"))

    def test_launched_at_is_earliest_line_for_the_same_tag_not_the_latest(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "worker-1.log"
            path.write_text(
                "[2026-07-20T19:00:00] round 1: attempting JPEG:APP12:CAM1\n"
                "[2026-07-20T19:00:05] [JPEG:APP12:CAM1] failed attempt 1/10\n"
                "[2026-07-20T19:05:00] round 2: attempting JPEG:APP12:CAM1\n"
            )
            round_num, tag, launched_at = parse_current_tag_progress(path)
            self.assertEqual(round_num, 2)  # most recent round number
            self.assertEqual(tag, "JPEG:APP12:CAM1")
            # launched_at anchors to the FIRST attempt on this tag, not the latest.
            self.assertEqual(launched_at, parse_timestamp("2026-07-20T19:00:00"))

    def test_stops_at_a_different_earlier_tag(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "worker-1.log"
            path.write_text(
                "[2026-07-20T18:00:00] round 1: attempting JPEG:APP12:CAM9\n"
                "[2026-07-20T19:00:00] round 2: attempting JPEG:APP12:CAM1\n"
            )
            round_num, tag, launched_at = parse_current_tag_progress(path)
            self.assertEqual(tag, "JPEG:APP12:CAM1")
            self.assertEqual(launched_at, parse_timestamp("2026-07-20T19:00:00"))


class ParseTimestampTests(unittest.TestCase):
    def test_naive_local_timestamp(self):
        self.assertIsNotNone(parse_timestamp("2026-07-20T19:00:00"))

    def test_aware_utc_timestamp_with_fractional_seconds(self):
        self.assertIsNotNone(parse_timestamp("2026-07-21T15:44:19.068866+00:00"))

    def test_garbage_returns_none(self):
        self.assertIsNone(parse_timestamp("not a timestamp"))


class FormatRelativeTests(unittest.TestCase):
    def test_none_is_never(self):
        self.assertEqual(format_relative(None), "never")

    def test_just_now(self):
        self.assertEqual(format_relative(2), "just now")

    def test_seconds(self):
        self.assertEqual(format_relative(45), "45s ago")

    def test_minutes(self):
        self.assertEqual(format_relative(150), "2m ago")

    def test_hours(self):
        self.assertEqual(format_relative(3 * 3600 + 1800), "3.5h ago")

    def test_days(self):
        self.assertEqual(format_relative(2 * 86400 + 43200), "2.5d ago")


class BlacklistStatsTests(unittest.TestCase):
    def test_empty_state(self):
        stats = blacklist_stats({}, now=1000.0)
        self.assertEqual(stats, {"total": 0, "last_hour": 0, "last_24h": 0, "per_worker": {}})

    def test_counts_by_age_window_and_worker(self):
        now = 1_000_000.0
        state = {
            "JPEG:A": {"blacklisted": True, "blacklisted_at": now - 100, "blacklisted_by": "1"},
            "JPEG:B": {"blacklisted": True, "blacklisted_at": now - 7200, "blacklisted_by": "1"},
            "JPEG:C": {"blacklisted": True, "blacklisted_at": now - 90000, "blacklisted_by": "2"},
            "JPEG:D": {"blacklisted": False, "fails": 3},
        }
        stats = blacklist_stats(state, now)
        self.assertEqual(stats["total"], 3)
        self.assertEqual(stats["last_hour"], 1)
        self.assertEqual(stats["last_24h"], 2)
        self.assertEqual(stats["per_worker"], {"1": 2, "2": 1})

    def test_missing_blacklisted_at_still_counts_toward_total_only(self):
        state = {"JPEG:A": {"blacklisted": True}}
        stats = blacklist_stats(state, now=1000.0)
        self.assertEqual(stats["total"], 1)
        self.assertEqual(stats["last_hour"], 0)
        self.assertEqual(stats["last_24h"], 0)


class TagIterationTests(unittest.TestCase):
    def test_no_tag_is_none(self):
        self.assertIsNone(tag_iteration({}, None))

    def test_never_attempted_tag_is_iteration_one(self):
        self.assertEqual(tag_iteration({}, "JPEG:A"), 1)

    def test_uses_persisted_fails_count_not_a_process_local_counter(self):
        # The exact scenario that motivated this: worker A fails once on a
        # tag and releases its claim; worker B picks the SAME tag up fresh
        # (its own round_num starts at 1 again) -- iteration must reflect
        # the tag's true cumulative history (2), not worker B's own fresh
        # round_num (which would wrongly read back as 1).
        state = {"JPEG:A": {"blacklisted": False, "fails": 1}}
        self.assertEqual(tag_iteration(state, "JPEG:A"), 2)


class ParseTagsFoundLogTests(unittest.TestCase):
    def test_missing_file_is_empty(self):
        self.assertEqual(parse_tags_found_log(Path("/nonexistent/tags-found.log")), [])

    def test_parses_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "tags-found.log"
            path.write_text(
                "2026-07-20T19:00:00 worker=1 tag=JPEG:EXIF:LensModel gaps_closed=1\n"
                "\n"
                "2026-07-20T19:05:00 worker=3 tag=JPEG:APP12:CAM1 gaps_closed=2\n"
            )
            entries = parse_tags_found_log(path)
            self.assertEqual(len(entries), 2)
            self.assertEqual(entries[0], ("2026-07-20T19:00:00", "1", "JPEG:EXIF:LensModel", 1))
            self.assertEqual(entries[1], ("2026-07-20T19:05:00", "3", "JPEG:APP12:CAM1", 2))


class FoundStatsTests(unittest.TestCase):
    def test_no_entries(self):
        stats = found_stats([], now=1000.0)
        self.assertEqual(stats["total"], 0)
        self.assertIsNone(stats["last_at"])
        self.assertIsNone(stats["last_tag"])

    def test_last_is_the_max_timestamp_not_the_last_list_entry(self):
        now = parse_timestamp("2026-07-20T20:00:00")
        entries = [
            ("2026-07-20T19:50:00", "2", "JPEG:LATER", 1),  # appended out of order
            ("2026-07-20T19:55:00", "1", "JPEG:ACTUAL_LAST", 1),
        ]
        stats = found_stats(entries, now)
        self.assertEqual(stats["last_tag"], "JPEG:ACTUAL_LAST")
        self.assertEqual(stats["last_worker"], "1")

    def test_window_counts(self):
        now = parse_timestamp("2026-07-20T20:00:00")
        entries = [
            ("2026-07-20T19:59:00", "1", "JPEG:A", 1),   # 1 min ago
            ("2026-07-20T18:30:00", "1", "JPEG:B", 1),   # 1.5 hours ago
            ("2026-07-19T10:00:00", "1", "JPEG:C", 1),   # >24h ago
        ]
        stats = found_stats(entries, now)
        self.assertEqual(stats["total"], 3)
        self.assertEqual(stats["last_hour"], 1)
        self.assertEqual(stats["last_24h"], 2)


class ParseWrapperLogTests(unittest.TestCase):
    def test_missing_file_is_empty(self):
        self.assertEqual(parse_wrapper_log(Path("/nonexistent/parallel-wrapper.log")), {})

    def test_first_start_is_zero_restarts(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "parallel-wrapper.log"
            path.write_text("[worker 1] started (pid 100), worktree /tmp/x\n")
            stats = parse_wrapper_log(path)
            self.assertEqual(stats["1"]["restarts"], 0)
            self.assertEqual(stats["1"]["crashes"], 0)

    def test_counts_restarts_beyond_the_first_and_crashes(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "parallel-wrapper.log"
            path.write_text(
                "[worker 1] started (pid 100), worktree /tmp/x\n"
                "[worker 1] exited (code 1) -- /tmp/worker-1.log\n"
                "[worker 1] CRASHED (exit code 1), attempt 1/5 -- see /tmp/worker-1.log -- respawning\n"
                "[worker 1] started (pid 101), worktree /tmp/x\n"
                "[worker 1] CRASHED 2 times in a row (exit code 1) -- giving up on this slot\n"
            )
            stats = parse_wrapper_log(path)
            self.assertEqual(stats["1"]["restarts"], 1)  # 2 starts total, minus the first
            self.assertEqual(stats["1"]["crashes"], 2)

    def test_tracks_multiple_workers_independently(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "parallel-wrapper.log"
            path.write_text(
                "[worker 1] started (pid 100), worktree /tmp/x\n"
                "[worker 2] started (pid 200), worktree /tmp/y\n"
                "[worker 2] started (pid 201), worktree /tmp/y\n"
            )
            stats = parse_wrapper_log(path)
            self.assertEqual(stats["1"]["restarts"], 0)
            self.assertEqual(stats["2"]["restarts"], 1)


class DiscoverFormatProgressTests(unittest.TestCase):
    def _write_report(self, path, by_format):
        path.write_text(json.dumps({"by_format": by_format}))

    def test_no_files_is_empty(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(discover_format_progress(Path(tmpdir)), {})

    def test_reads_matched_and_total_from_a_single_format_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            self._write_report(
                tmp / "tagcmp-JPEG.json",
                {"JPEG": {"matched_tags": ["a", "b", "c"], "total_exiftool_tags": 10}},
            )
            progress = discover_format_progress(tmp)
            self.assertEqual(progress["JPEG"]["matched"], 3)
            self.assertEqual(progress["JPEG"]["total"], 10)

    def test_zero_total_format_is_skipped(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            self._write_report(tmp / "tagcmp-EMPTY.json", {"EMPTY": {"matched_tags": [], "total_exiftool_tags": 0}})
            self.assertEqual(discover_format_progress(tmp), {})

    def test_newer_file_wins_over_an_older_one_for_the_same_format(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            old_path = tmp / "tagcmp-old-test.json"
            new_path = tmp / "tagcmp-JPEG.json"
            self._write_report(old_path, {"JPEG": {"matched_tags": [1], "total_exiftool_tags": 100}})
            self._write_report(new_path, {"JPEG": {"matched_tags": [1, 2], "total_exiftool_tags": 100}})
            # Force a real, unambiguous mtime ordering.
            old_time = time.time() - 1000
            import os
            os.utime(old_path, (old_time, old_time))
            progress = discover_format_progress(tmp)
            self.assertEqual(progress["JPEG"]["matched"], 2)
            self.assertEqual(progress["JPEG"]["source"], new_path)

    def test_reads_full_corpus_comparison_json_from_repo_root(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            self._write_report(tmp / "comparison.json", {"NEF": {"matched_tags": [1, 2], "total_exiftool_tags": 4}})
            progress = discover_format_progress(tmp / "nonexistent-tagcmp-dir", repo_root=tmp)
            self.assertEqual(progress["NEF"]["matched"], 2)
            self.assertEqual(progress["NEF"]["total"], 4)

    def test_matched_tags_as_a_plain_int_is_also_accepted(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            self._write_report(tmp / "tagcmp-JPEG.json", {"JPEG": {"matched_tags": 5, "total_exiftool_tags": 10}})
            progress = discover_format_progress(tmp)
            self.assertEqual(progress["JPEG"]["matched"], 5)


class BarColorTests(unittest.TestCase):
    def test_full_is_bright_green(self):
        self.assertEqual(bar_color(100), BRIGHT_GREEN)

    def test_zero_is_red(self):
        self.assertEqual(bar_color(0), RED)


class RenderProgressBarTests(unittest.TestCase):
    def test_includes_counts_and_percentage(self):
        rendered = render_progress_bar(629, 3690, width=40)
        self.assertIn("629", rendered)
        self.assertIn("3690", rendered)
        self.assertIn("17.0%", rendered)

    def test_zero_total_does_not_crash(self):
        rendered = render_progress_bar(0, 0, width=10)
        self.assertIn("0", rendered)

    def test_full_bar_gets_a_checkmark(self):
        rendered = render_progress_bar(10, 10, width=10)
        self.assertIn("✓", rendered)


class RenderFormatProgressTests(unittest.TestCase):
    def test_empty_progress_shows_explanatory_line(self):
        lines = render_format_progress({})
        self.assertEqual(len(lines), 1)
        self.assertIn("no tag-comparison data", lines[0])

    def test_least_complete_format_sorts_first(self):
        progress = {
            "DONE": {"matched": 10, "total": 10, "mtime": 0, "source": Path("x")},
            "JPEG": {"matched": 1, "total": 100, "mtime": 0, "source": Path("y")},
        }
        lines = render_format_progress(progress)
        jpeg_index = next(i for i, l in enumerate(lines) if "JPEG" in l)
        done_index = next(i for i, l in enumerate(lines) if "DONE" in l)
        self.assertLess(jpeg_index, done_index)


class DiscoverFormatsTests(unittest.TestCase):
    def test_lists_log_stems_sorted(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "NEF.log").write_text("")
            (tmp / "AVI.log").write_text("")
            (tmp / "not-a-log.txt").write_text("")
            self.assertEqual(discover_formats(tmp), ["AVI", "NEF"])


class FindActiveLogDirTests(unittest.TestCase):
    def test_returns_none_when_no_candidate_has_logs(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            empty_a, empty_b = tmp / "a", tmp / "b"
            empty_a.mkdir()
            self.assertIsNone(find_active_log_dir([empty_a, empty_b]))

    def test_picks_the_only_candidate_with_logs(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            with_logs, without_logs = tmp / "a", tmp / "b"
            with_logs.mkdir()
            without_logs.mkdir()
            (with_logs / "NEF.log").write_text("")
            self.assertEqual(find_active_log_dir([without_logs, with_logs]), with_logs)

    def test_prefers_the_more_recently_modified_candidate(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            stale, fresh = tmp / "stale", tmp / "fresh"
            stale.mkdir()
            fresh.mkdir()
            stale_log = stale / "NEF.log"
            stale_log.write_text("")
            time.sleep(0.01)
            fresh_log = fresh / "worker-1.log"
            fresh_log.write_text("")
            self.assertEqual(find_active_log_dir([stale, fresh]), fresh)
            self.assertEqual(find_active_log_dir([fresh, stale]), fresh)


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

    def test_without_explicit_log_dir_auto_detects_between_the_two_wrapper_defaults(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            oxidex_home = Path(tmpdir)
            model_fix_dir = oxidex_home / "logs" / "parallel-model-fix"
            tag_fix_dir = oxidex_home / "logs" / "parallel-tag-fix"
            model_fix_dir.mkdir(parents=True)
            sleeps = []

            def fake_sleep(interval):
                sleeps.append(interval)
                if len(sleeps) == 1:
                    (model_fix_dir / "NEF.log").write_text("[NEF] gaps 5 -> 2\n")
                elif len(sleeps) == 2:
                    raise KeyboardInterrupt

            out = io.StringIO()
            with patch("watch_parallel_fix.OXIDEX_HOME", oxidex_home):
                exit_code = main(["--interval", "0.1"], sleep_fn=fake_sleep, stdout=out)

            self.assertEqual(exit_code, 0)
            self.assertIn(str(tag_fix_dir), out.getvalue())
            self.assertIn(str(model_fix_dir), out.getvalue())
            self.assertIn("NEF", out.getvalue())


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


class DiscoverWorkerIdsTests(unittest.TestCase):
    def test_tag_mode_from_worker_n_logs(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text("")
            (tmp / "worker-2.log").write_text("")
            self.assertEqual(discover_worker_ids(tmp), ([1, 2], "tag"))

    def test_format_mode_from_format_logs(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "JPEG.log").write_text("")
            (tmp / "NEF.log").write_text("")
            self.assertEqual(discover_worker_ids(tmp), (["JPEG", "NEF"], "format"))

    def test_empty_dir_is_format_mode_with_no_ids(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(discover_worker_ids(Path(tmpdir)), ([], "format"))


class WorkerWorktreeNameTests(unittest.TestCase):
    def test_tag_mode_uses_numeric_worker_naming(self):
        self.assertEqual(worker_worktree_name(3, "tag"), "model-fix-tag-worker-3")

    def test_format_mode_uses_lowercase_format_naming(self):
        self.assertEqual(worker_worktree_name("JPEG", "format"), "model-fix-jpeg")


class WorkerLogPathTests(unittest.TestCase):
    def test_tag_mode_path(self):
        self.assertEqual(worker_log_path(Path("/logs"), 3, "tag"), Path("/logs/worker-3.log"))

    def test_format_mode_path(self):
        self.assertEqual(worker_log_path(Path("/logs"), "JPEG", "format"), Path("/logs/JPEG.log"))


class RenderDashboardTests(unittest.TestCase):
    def test_includes_header_stats_and_worker_row(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text(
                "[2026-07-20T19:00:00] round 1: attempting JPEG:APP12:CAM1\n"
                "[JPEG:APP12:CAM1] FIXED\n"
            )
            tags_found_log = tmp / "tags-found.log"
            tags_found_log.write_text("2026-07-20T19:00:00 worker=1 tag=JPEG:APP12:CAM1 gaps_closed=1\n")
            tag_state_path = tmp / "state.json"
            tag_state_path.write_text(json.dumps({}))
            wrapper_log_path = tmp / "wrapper.log"
            wrapper_log_path.write_text("[worker 1] started (pid 1), worktree /tmp/x\n")

            now = parse_timestamp("2026-07-20T19:05:00")
            output = render_dashboard(
                tmp, [1], tags_found_log, tag_state_path, wrapper_log_path,
                format_progress={}, max_tag_fails=10, now=now,
            )
            self.assertIn("OXIDEX TAG-FIX DASHBOARD", output)
            self.assertIn("Tags found:", output)
            self.assertIn("Blacklisted:", output)
            self.assertIn("worker-1", output)
            self.assertIn("JPEG:APP12:CAM1", output)
            self.assertIn("restarts:", output)

    def test_no_workers_shows_explanatory_line(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            output = render_dashboard(
                tmp, [], tmp / "tags-found.log", tmp / "state.json", tmp / "wrapper.log",
                format_progress={}, max_tag_fails=10, now=time.time(),
            )
            self.assertIn("no workers found", output)

    def test_omitting_worktree_dir_skips_the_model_line(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text("round 1: attempting JPEG:APP12:CAM1\n")
            output = render_dashboard(
                tmp, [1], tmp / "tags-found.log", tmp / "state.json", tmp / "wrapper.log",
                format_progress={}, max_tag_fails=10, now=time.time(),
            )
            self.assertNotIn("Fixer:", output)

    def test_worktree_dir_adds_the_fixer_and_reviewer_model_line(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text("round 1: attempting JPEG:APP12:CAM1\n")
            worktree_dir = Path(tmpdir) / "worktrees"
            worker_dir = worktree_dir / "model-fix-tag-worker-1"
            worker_dir.mkdir(parents=True)
            (worker_dir / "config.toml").write_text(
                '[worker]\nreasoning_effort = "max"\n[[worker.models]]\nname = "gpt-5.6-sol"\n'
            )
            output = render_dashboard(
                tmp, [1], tmp / "tags-found.log", tmp / "state.json", tmp / "wrapper.log",
                format_progress={}, max_tag_fails=10, now=time.time(), worktree_dir=worktree_dir,
            )
            self.assertIn("Fixer:", output)
            self.assertIn("gpt-5.6-sol", output)
            self.assertIn("@max", output)
            self.assertIn("Reviewer:", output)

    def test_format_mode_renders_the_same_rich_dashboard(self):
        # parallel_model_fix_loop.py's workers run the exact same
        # run_tag_loop machinery as parallel_tag_fix_loop.py's (see
        # model_fix_loop.py's main -- --only-format just filters which
        # gaps are considered), so mode="format" should get everything
        # mode="tag" gets: round/tag tracking, blacklist stats, model
        # config, request stats -- just keyed by format name instead of
        # a numeric worker id.
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "JPEG.log").write_text(
                "[2026-07-21T10:00:00] round 1: attempting JPEG:APP12:ImageSize\n"
            )
            worktree_dir = tmp / "worktrees"
            worker_dir = worktree_dir / "model-fix-jpeg"
            worker_dir.mkdir(parents=True)
            (worker_dir / "config.toml").write_text(
                '[worker]\nreasoning_effort = "max"\n[[worker.models]]\nname = "gpt-5.6-sol"\n'
            )
            manifest_path = tmp / "model-fix-requests" / "manifest.log"
            manifest_path.parent.mkdir(parents=True, exist_ok=True)
            manifest_path.write_text(
                "2026-07-21T10:00:01 phase=fixer worker=JPEG model=gpt-5.6-sol prompt_chars=100 "
                "elapsed=12.0s reply_chars=10 OK\n"
            )
            output = render_dashboard(
                tmp, ["JPEG"], tmp / "tags-found.log", tmp / "state.json", tmp / "wrapper.log",
                format_progress={}, max_tag_fails=10, now=time.time(), worktree_dir=worktree_dir,
                manifest_path=manifest_path, mode="format",
            )
            self.assertIn("OXIDEX TAG-FIX DASHBOARD", output)
            self.assertIn("JPEG", output)
            self.assertIn("JPEG:APP12:ImageSize", output)
            self.assertIn("Fixer:", output)
            self.assertIn("gpt-5.6-sol", output)
            self.assertIn("Requests:", output)
            self.assertNotIn("worker-JPEG", output)  # format-mode row label has no "worker-" prefix


class LoadTagStateTests(unittest.TestCase):
    def test_missing_file_is_empty_dict(self):
        self.assertEqual(load_tag_state(Path("/nonexistent/state.json")), {})

    def test_loads_real_json(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "state.json"
            path.write_text(json.dumps({"JPEG:A": {"blacklisted": True}}))
            self.assertEqual(load_tag_state(path), {"JPEG:A": {"blacklisted": True}})


class LoadWorkerModelConfigTests(unittest.TestCase):
    def _write_config(self, worktree_dir, worker_id, toml_text):
        worker_dir = Path(worktree_dir) / f"model-fix-tag-worker-{worker_id}"
        worker_dir.mkdir(parents=True, exist_ok=True)
        (worker_dir / "config.toml").write_text(toml_text)

    def test_missing_worktree_returns_all_none(self):
        result = load_worker_model_config("/nonexistent", 1)
        self.assertEqual(result, (None, None, None, None))

    def test_reads_fixer_and_reviewer_pools_and_reasoning(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self._write_config(
                tmpdir, 1,
                '[worker]\nreasoning_effort = "max"\n'
                '[[worker.models]]\nname = "gpt-5.6-sol"\n'
                '[reviewer]\nreasoning_effort = "high"\n'
                '[[reviewer.models]]\nname = "gpt-5.6-sol"\n',
            )
            fixer_models, fixer_reasoning, reviewer_models, reviewer_reasoning = load_worker_model_config(
                tmpdir, 1
            )
            self.assertEqual(fixer_models, ["gpt-5.6-sol"])
            self.assertEqual(fixer_reasoning, "max")
            self.assertEqual(reviewer_models, ["gpt-5.6-sol"])
            self.assertEqual(reviewer_reasoning, "high")

    def test_reviewer_falls_back_to_worker_when_absent(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self._write_config(
                tmpdir, 2,
                '[worker]\nreasoning_effort = "low"\n'
                '[[worker.models]]\nname = "glm5.2-fast"\n'
                '[[worker.models]]\nname = "Kimi-K2.6"\n',
            )
            fixer_models, fixer_reasoning, reviewer_models, reviewer_reasoning = load_worker_model_config(
                tmpdir, 2
            )
            self.assertEqual(fixer_models, ["glm5.2-fast", "Kimi-K2.6"])
            self.assertEqual(reviewer_models, fixer_models)
            self.assertEqual(reviewer_reasoning, "low")

    def test_table_entry_models_use_their_name_field(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self._write_config(
                tmpdir, 3,
                '[worker]\n'
                '[[worker.models]]\n'
                'name = "accounts/fireworks/routers/kimi-k2p7-code-fast"\n'
                'base_url = "https://api.fireworks.ai/inference/v1"\n',
            )
            fixer_models, _, _, _ = load_worker_model_config(tmpdir, 3)
            self.assertEqual(fixer_models, ["accounts/fireworks/routers/kimi-k2p7-code-fast"])


class ParseManifestLogTests(unittest.TestCase):
    def test_missing_file_is_empty(self):
        self.assertEqual(parse_manifest_log(Path("/nonexistent/manifest.log")), [])

    def test_parses_ok_and_error_lines_skips_retry_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "manifest.log"
            path.write_text(
                "2026-07-21T10:00:00 phase=fixer worker=1 model=gpt-5.6-sol prompt_chars=1200 "
                "elapsed=12.3s reply_chars=500 OK\n"
                "2026-07-21T10:05:00 phase=fixer worker=1 model=gpt-5.6-sol RETRY model call retry "
                "1/1000 after RuntimeError('empty reply'), waiting 2s\n"
                "2026-07-21T10:06:00 phase=reviewer worker=2 model=gpt-5.6-sol prompt_chars=200 "
                "elapsed=1.5s reply_chars=10 OK\n"
                "2026-07-21T10:10:00 phase=fixer worker=JPEG model=gpt-5.6-sol prompt_chars=900 "
                "elapsed=45.0s ERROR=<urlopen error DNS failure>\n"
            )
            entries = parse_manifest_log(path)
            # 3 completed calls (OK/ERROR); the RETRY line is excluded --
            # it has no elapsed time of its own to report a latency for.
            self.assertEqual(len(entries), 3)
            self.assertEqual(entries[0], ("2026-07-21T10:00:00", "fixer", 12.3, True, "1"))
            self.assertEqual(entries[1], ("2026-07-21T10:06:00", "reviewer", 1.5, True, "2"))
            self.assertEqual(entries[2], ("2026-07-21T10:10:00", "fixer", 45.0, False, "JPEG"))


class EntriesForWorkerTests(unittest.TestCase):
    def test_filters_by_worker_label_and_drops_the_tag(self):
        entries = [
            ("2026-07-21T10:00:00", "fixer", 12.3, True, "1"),
            ("2026-07-21T10:06:00", "reviewer", 1.5, True, "2"),
            ("2026-07-21T10:10:00", "fixer", 45.0, False, "JPEG"),
        ]
        self.assertEqual(entries_for_worker(entries, 1), [("2026-07-21T10:00:00", "fixer", 12.3, True)])
        self.assertEqual(entries_for_worker(entries, "JPEG"), [("2026-07-21T10:10:00", "fixer", 45.0, False)])
        self.assertEqual(entries_for_worker(entries, 99), [])


class RequestStatsTests(unittest.TestCase):
    def test_empty_entries(self):
        stats = request_stats([])
        self.assertEqual(stats["fixer"], {"count": 0, "mean": None, "median": None})
        self.assertEqual(stats["reviewer"], {"count": 0, "mean": None, "median": None})
        self.assertIsNone(stats["last"])

    def test_computes_mean_and_median_per_phase(self):
        entries = [
            ("2026-07-21T10:00:00", "fixer", 10.0, True),
            ("2026-07-21T10:01:00", "fixer", 20.0, True),
            ("2026-07-21T10:02:00", "fixer", 30.0, False),
            ("2026-07-21T10:03:00", "reviewer", 5.0, True),
        ]
        stats = request_stats(entries)
        self.assertEqual(stats["fixer"]["count"], 3)
        self.assertEqual(stats["fixer"]["mean"], 20.0)
        self.assertEqual(stats["fixer"]["median"], 20.0)
        self.assertEqual(stats["reviewer"]["count"], 1)
        self.assertEqual(stats["reviewer"]["mean"], 5.0)

    def test_last_is_the_most_recent_entry_of_either_phase(self):
        entries = [
            ("2026-07-21T10:00:00", "fixer", 10.0, True),
            ("2026-07-21T10:05:00", "reviewer", 3.0, True),
        ]
        stats = request_stats(entries)
        self.assertEqual(stats["last"]["phase"], "reviewer")
        self.assertEqual(stats["last"]["elapsed"], 3.0)

    def test_since_filters_out_earlier_entries(self):
        entries = [
            ("2026-07-21T10:00:00", "fixer", 10.0, True),
            ("2026-07-21T10:10:00", "fixer", 20.0, True),
        ]
        since = parse_timestamp("2026-07-21T10:05:00")
        stats = request_stats(entries, since=since)
        self.assertEqual(stats["fixer"]["count"], 1)
        self.assertEqual(stats["fixer"]["mean"], 20.0)

    def test_since_none_includes_everything(self):
        entries = [
            ("2026-07-21T10:00:00", "fixer", 10.0, True),
            ("2026-07-21T10:10:00", "fixer", 20.0, True),
        ]
        stats = request_stats(entries, since=None)
        self.assertEqual(stats["fixer"]["count"], 2)


class ParseCurrentRoundStartTests(unittest.TestCase):
    def test_missing_file_is_none(self):
        self.assertIsNone(parse_current_round_start(Path("/nonexistent/worker-1.log")))

    def test_no_round_line_yet_is_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "worker-1.log"
            path.write_text("   Compiling oxidex v1.2.1\n")
            self.assertIsNone(parse_current_round_start(path))

    def test_returns_the_most_recent_rounds_own_timestamp(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "worker-1.log"
            path.write_text(
                "[2026-07-21T10:00:00] round 1: attempting JPEG:APP12:CAM1\n"
                "[2026-07-21T10:00:05] [JPEG:APP12:CAM1] failed attempt 1/10\n"
                "[2026-07-21T10:05:00] round 2: attempting JPEG:APP12:CAM1\n"
            )
            # Unlike parse_current_tag_progress's launched_at (anchored to
            # the earliest same-tag line), this is the LATEST round's own
            # start time.
            self.assertEqual(parse_current_round_start(path), parse_timestamp("2026-07-21T10:05:00"))


class RenderDashboardRequestStatsTests(unittest.TestCase):
    def test_includes_aggregate_and_per_worker_request_stats(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text(
                "[2026-07-21T10:00:00] round 1: attempting JPEG:APP12:CAM1\n"
            )
            manifest_path = tmp / "model-fix-requests" / "manifest.log"
            manifest_path.parent.mkdir(parents=True, exist_ok=True)
            manifest_path.write_text(
                "2026-07-21T10:00:01 phase=fixer worker=1 model=gpt-5.6-sol prompt_chars=100 "
                "elapsed=12.0s reply_chars=10 OK\n"
                "2026-07-21T10:00:30 phase=reviewer worker=1 model=gpt-5.6-sol prompt_chars=50 "
                "elapsed=3.0s reply_chars=5 OK\n"
            )
            now = parse_timestamp("2026-07-21T10:01:00")
            output = render_dashboard(
                tmp, [1], tmp / "tags-found.log", tmp / "state.json", tmp / "wrapper.log",
                format_progress={}, max_tag_fails=10, now=now, manifest_path=manifest_path,
            )
            self.assertIn("API requests:", output)
            self.assertIn("Requests:", output)
            self.assertIn("this round:", output)
            self.assertIn("2", output)  # aggregate: 1 fixer + 1 reviewer request seen somewhere

    def test_entries_from_other_workers_dont_pollute_this_workers_row(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text(
                "[2026-07-21T10:00:00] round 1: attempting JPEG:APP12:CAM1\n"
            )
            manifest_path = tmp / "model-fix-requests" / "manifest.log"
            manifest_path.parent.mkdir(parents=True, exist_ok=True)
            manifest_path.write_text(
                "2026-07-21T10:00:01 phase=fixer worker=1 model=gpt-5.6-sol prompt_chars=100 "
                "elapsed=12.0s reply_chars=10 OK\n"
                "2026-07-21T10:00:30 phase=fixer worker=2 model=gpt-5.6-sol prompt_chars=50 "
                "elapsed=99.0s reply_chars=5 OK\n"
            )
            output = render_dashboard(
                tmp, [1], tmp / "tags-found.log", tmp / "state.json", tmp / "wrapper.log",
                format_progress={}, max_tag_fails=10, now=time.time(), manifest_path=manifest_path,
            )
            # worker 1's own Requests: line must reflect just its 1 fixer call
            # (mean/median 12.0s) -- not worker 2's 99.0s call bleeding in now
            # that both share one manifest.log. The aggregate API requests:
            # line legitimately mentions 99.0s (it's the most recent call
            # overall), so assert on worker 1's own "Requests:" line specifically.
            requests_line = next(line for line in output.splitlines() if "Requests:" in line)
            self.assertIn("1", requests_line)
            self.assertIn("12.0s", requests_line)
            self.assertNotIn("99.0s", requests_line)

    def test_no_manifest_path_skips_the_requests_line(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "worker-1.log").write_text("round 1: attempting JPEG:APP12:CAM1\n")
            output = render_dashboard(
                tmp, [1], tmp / "tags-found.log", tmp / "state.json", tmp / "wrapper.log",
                format_progress={}, max_tag_fails=10, now=time.time(),
            )
            self.assertNotIn("Requests:", output)


class MainLoopWorkerModeTests(unittest.TestCase):
    def test_auto_detects_worker_mode_and_renders_the_dashboard(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            sleeps = []

            def fake_sleep(interval):
                sleeps.append(interval)
                if len(sleeps) == 1:
                    (tmp / "worker-1.log").write_text(
                        "[2026-07-20T19:00:00] round 1: attempting JPEG:EXIF:LensModel\n"
                    )
                    (tmp / "tags-found.log").write_text("2026-07-20T19:00:00 worker=1 tag=Y gaps_closed=1\n")
                elif len(sleeps) == 2:
                    raise KeyboardInterrupt

            out = io.StringIO()
            exit_code = main(
                ["--log-dir", str(tmp), "--tags-found-log", str(tmp / "tags-found.log"), "--interval", "0.1"],
                sleep_fn=fake_sleep, stdout=out,
            )

            self.assertEqual(exit_code, 0)
            self.assertIn("worker-1", out.getvalue())
            self.assertIn("OXIDEX TAG-FIX DASHBOARD", out.getvalue())
            self.assertIn("Tags found:", out.getvalue())

    def test_auto_detects_format_mode_from_parallel_model_fix_loop_logs(self):
        # The same rich dashboard main() renders for parallel_tag_fix_loop.py's
        # worker-<N>.log runs must also come up for parallel_model_fix_loop.py's
        # <FORMAT>.log runs -- no separate, poorer fallback view.
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            sleeps = []

            def fake_sleep(interval):
                sleeps.append(interval)
                if len(sleeps) == 1:
                    (tmp / "JPEG.log").write_text(
                        "[2026-07-20T19:00:00] round 1: attempting JPEG:APP12:ImageSize\n"
                    )
                elif len(sleeps) == 2:
                    raise KeyboardInterrupt

            out = io.StringIO()
            exit_code = main(["--log-dir", str(tmp), "--interval", "0.1"], sleep_fn=fake_sleep, stdout=out)

            self.assertEqual(exit_code, 0)
            self.assertIn("OXIDEX TAG-FIX DASHBOARD", out.getvalue())
            self.assertIn("JPEG", out.getvalue())
            self.assertIn("JPEG:APP12:ImageSize", out.getvalue())


if __name__ == "__main__":
    unittest.main()
