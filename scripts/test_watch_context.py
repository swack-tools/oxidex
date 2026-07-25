import io
import json
import tempfile
import unittest
from pathlib import Path

from watch_context import (
    clamp_index,
    compute_max_scroll,
    format_elapsed_ago,
    handle_navigation_key,
    latest_calls_per_worker,
    load_manifest_entries,
    load_request_messages,
    load_response_text,
    main,
    parse_manifest_line,
    render_call_summary,
    render_interactive_frame,
    render_overview,
    render_worker_detail,
    request_path_for,
    response_path_for,
    strip_ansi,
)

FAKE_KEY_CODES = {
    "left": 1, "right": 2, "up": 3, "down": 4,
    "page_up": 5, "page_down": 6, "quit": 7, "toggle_phase": 8,
    "vim_up": 9, "vim_down": 10,
}


class ArtifactPathResolutionTests(unittest.TestCase):
    """model_fix_loop.py names request/response artifacts
    {ts}-{worker}-{phase}-... (worker-id isolation); files from before
    that change are {ts}-{phase}-... -- the resolvers prefer the tagged
    name on disk and fall back to the legacy one."""

    def _entry(self):
        return {"ts": "2026-07-24T10:00:00", "phase": "fixer", "worker": "NEF",
                "model": "m", "status": "OK"}

    def test_prefers_the_worker_tagged_file_when_present(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            tagged = req_log_dir / "2026-07-24T10:00:00-NEF-fixer-request.json"
            tagged.write_text("{}")
            self.assertEqual(request_path_for(req_log_dir, self._entry()), tagged)

    def test_falls_back_to_the_legacy_untagged_name(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            self.assertEqual(
                response_path_for(req_log_dir, self._entry()),
                req_log_dir / "2026-07-24T10:00:00-fixer-response.txt",
            )

    def test_same_second_calls_from_two_workers_resolve_to_distinct_files(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            for worker in ("NEF", "JPEG"):
                (req_log_dir / f"2026-07-24T10:00:00-{worker}-fixer-request.json").write_text("{}")
            nef = request_path_for(req_log_dir, self._entry())
            jpeg = request_path_for(req_log_dir, dict(self._entry(), worker="JPEG"))
            self.assertNotEqual(nef, jpeg)
            self.assertIn("NEF", nef.name)
            self.assertIn("JPEG", jpeg.name)


class ParseManifestLineTests(unittest.TestCase):
    def test_parses_ok_line(self):
        entry = parse_manifest_line(
            "2026-07-23T11:15:46 phase=fixer worker=MachO model=gpt-5.6-sol "
            "prompt_chars=71142 elapsed=220.4s reply_chars=3366 OK"
        )
        self.assertEqual(entry, {
            "ts": "2026-07-23T11:15:46", "phase": "fixer", "worker": "MachO",
            "model": "gpt-5.6-sol", "status": "OK", "prompt_chars": 71142,
            "elapsed": 220.4, "reply_chars": 3366,
        })

    def test_parses_retry_line(self):
        entry = parse_manifest_line(
            "2026-07-23T11:18:39 phase=fixer worker=ISO model=gpt-5.6-sol "
            "RETRY model call retry 1/1000 after RuntimeError('empty reply'), waiting 2s"
        )
        self.assertEqual(entry["status"], "RETRY")
        self.assertEqual(entry["worker"], "ISO")
        self.assertIn("empty reply", entry["detail"])

    def test_parses_error_line(self):
        entry = parse_manifest_line(
            "2026-07-23T11:00:00 phase=reviewer worker=RW2 model=gpt-5.6-sol "
            "prompt_chars=100 elapsed=5.0s ERROR=Connection refused"
        )
        self.assertEqual(entry["status"], "ERROR")
        self.assertEqual(entry["detail"], "Connection refused")

    def test_malformed_line_returns_none(self):
        self.assertIsNone(parse_manifest_line("not a manifest line at all"))
        self.assertIsNone(parse_manifest_line(""))


class LoadManifestEntriesTests(unittest.TestCase):
    def test_missing_file_returns_empty_list(self):
        self.assertEqual(load_manifest_entries(Path("/nonexistent/manifest.log")), [])

    def test_reads_and_parses_lines_skipping_malformed(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "manifest.log"
            path.write_text(
                "garbage\n"
                "2026-07-23T11:00:00 phase=fixer worker=A model=m prompt_chars=1 elapsed=1.0s reply_chars=1 OK\n"
            )
            entries = load_manifest_entries(path)
        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0]["worker"], "A")


class LatestCallsPerWorkerTests(unittest.TestCase):
    def test_tracks_most_recent_per_worker_per_phase(self):
        entries = [
            {"ts": "t1", "phase": "fixer", "worker": "A", "status": "OK"},
            {"ts": "t2", "phase": "fixer", "worker": "A", "status": "RETRY"},
            {"ts": "t3", "phase": "reviewer", "worker": "A", "status": "OK"},
            {"ts": "t4", "phase": "fixer", "worker": "B", "status": "OK"},
        ]
        result = latest_calls_per_worker(entries)
        self.assertEqual(result["A"]["fixer"]["ts"], "t2")  # last one wins
        self.assertEqual(result["A"]["reviewer"]["ts"], "t3")
        self.assertEqual(result["B"]["fixer"]["ts"], "t4")
        self.assertIsNone(result["B"]["reviewer"])

    def test_empty_entries_returns_empty_dict(self):
        self.assertEqual(latest_calls_per_worker([]), {})

    def test_retry_does_not_overwrite_the_content_entry(self):
        entries = [
            {"ts": "t1", "phase": "fixer", "worker": "A", "status": "OK"},
            {"ts": "t2", "phase": "fixer", "worker": "A", "status": "RETRY"},
            {"ts": "t3", "phase": "fixer", "worker": "A", "status": "RETRY"},
        ]
        result = latest_calls_per_worker(entries)
        self.assertEqual(result["A"]["fixer"]["ts"], "t3")          # live status
        self.assertEqual(result["A"]["fixer_content"]["ts"], "t1")  # last completed

    def test_ok_and_error_entries_update_the_content_entry(self):
        entries = [
            {"ts": "t1", "phase": "fixer", "worker": "A", "status": "OK"},
            {"ts": "t2", "phase": "fixer", "worker": "A", "status": "ERROR"},
        ]
        result = latest_calls_per_worker(entries)
        self.assertEqual(result["A"]["fixer_content"]["ts"], "t2")


class LoadRequestResponseTests(unittest.TestCase):
    def test_load_request_messages_reads_messages_list(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "2026-07-23T11:00:00-fixer-request.json").write_text(
                json.dumps({"messages": [{"role": "user", "content": "hello"}]})
            )
            entry = {"ts": "2026-07-23T11:00:00", "phase": "fixer"}
            messages = load_request_messages(req_log_dir, entry)
        self.assertEqual(messages, [{"role": "user", "content": "hello"}])

    def test_load_request_messages_missing_file_returns_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            entry = {"ts": "2026-07-23T11:00:00", "phase": "fixer"}
            self.assertIsNone(load_request_messages(Path(tmpdir), entry))

    def test_load_response_text_reads_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "2026-07-23T11:00:00-fixer-response.txt").write_text("```diff\n+x\n```")
            entry = {"ts": "2026-07-23T11:00:00", "phase": "fixer"}
            self.assertEqual(load_response_text(req_log_dir, entry), "```diff\n+x\n```")

    def test_load_response_text_missing_file_returns_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            entry = {"ts": "2026-07-23T11:00:00", "phase": "fixer"}
            self.assertIsNone(load_response_text(Path(tmpdir), entry))


class FormatElapsedAgoTests(unittest.TestCase):
    def test_seconds_ago(self):
        base = 1_700_000_000
        ts_str = __import__("time").strftime("%Y-%m-%dT%H:%M:%S", __import__("time").localtime(base))
        self.assertEqual(format_elapsed_ago(ts_str, now_fn=lambda: base + 30), "30s ago")

    def test_minutes_ago(self):
        base = 1_700_000_000
        ts_str = __import__("time").strftime("%Y-%m-%dT%H:%M:%S", __import__("time").localtime(base))
        self.assertEqual(format_elapsed_ago(ts_str, now_fn=lambda: base + 300), "5m ago")

    def test_invalid_timestamp_returns_question_mark(self):
        self.assertEqual(format_elapsed_ago("not-a-timestamp"), "?")


class RenderCallSummaryTests(unittest.TestCase):
    def test_none_entry(self):
        self.assertIn("none yet", strip_ansi(render_call_summary(None)))

    def test_ok_entry_shows_sizes_and_elapsed(self):
        entry = {"status": "OK", "ts": "2026-07-23T11:00:00", "model": "m",
                  "prompt_chars": 1234, "reply_chars": 56, "elapsed": 2.5}
        rendered = strip_ansi(render_call_summary(entry, now_fn=lambda: 0))
        self.assertIn("1,234c", rendered)
        self.assertIn("56c", rendered)
        self.assertIn("2.5s", rendered)


class RenderOverviewTests(unittest.TestCase):
    def test_empty_shows_placeholder(self):
        self.assertIn("No model calls logged yet", strip_ansi(render_overview({}, 100)))

    def test_lists_workers_sorted(self):
        latest = {
            "RW2": {"fixer": {"status": "OK", "ts": "t", "model": "m", "prompt_chars": 1, "reply_chars": 1, "elapsed": 1.0}, "reviewer": None},
            "JPEG": {"fixer": None, "reviewer": None},
        }
        rendered = strip_ansi(render_overview(latest, 100, now_fn=lambda: 0))
        jpeg_idx = rendered.index("JPEG")
        rw2_idx = rendered.index("RW2")
        self.assertLess(jpeg_idx, rw2_idx)  # sorted alphabetically


class RenderWorkerDetailTests(unittest.TestCase):
    def test_no_call_for_worker(self):
        rendered = strip_ansi(render_worker_detail("X", None, Path("/nonexistent-req-log-dir"), "fixer", 100))
        self.assertIn("No fixer call logged yet", rendered)

    def test_shows_sent_and_received_sections(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "2026-07-23T11:00:00-fixer-request.json").write_text(
                json.dumps({"messages": [{"role": "user", "content": "fix this gap"}]})
            )
            (req_log_dir / "2026-07-23T11:00:00-fixer-response.txt").write_text("```diff\n+fix\n```")
            calls = {"fixer": {"status": "OK", "ts": "2026-07-23T11:00:00", "phase": "fixer", "model": "m"}}
            rendered = strip_ansi(
                render_worker_detail("JPEG", calls, req_log_dir, "fixer", 100, now_fn=lambda: 0)
            )
        self.assertIn("=== SENT", rendered)
        self.assertIn("fix this gap", rendered)
        self.assertIn("=== RECEIVED", rendered)
        self.assertIn("+fix", rendered)

    def test_retry_with_no_request_file_falls_back_to_last_completed_call(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "2026-07-23T11:00:00-fixer-request.json").write_text(
                json.dumps({"messages": [{"role": "user", "content": "the real conversation"}]})
            )
            (req_log_dir / "2026-07-23T11:00:00-fixer-response.txt").write_text("the real reply")
            calls = {
                "fixer": {"status": "RETRY", "ts": "2026-07-23T11:05:00", "phase": "fixer", "model": "m"},
                "fixer_content": {"status": "OK", "ts": "2026-07-23T11:00:00", "phase": "fixer", "model": "m"},
            }
            rendered = strip_ansi(
                render_worker_detail("ISO", calls, req_log_dir, "fixer", 100, now_fn=lambda: 0)
            )
        self.assertIn("showing the last completed fixer call", rendered)
        self.assertIn("the real conversation", rendered)
        self.assertIn("the real reply", rendered)
        self.assertNotIn("Request file missing", rendered)

    def test_retry_with_no_content_entry_still_reports_missing_request(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            calls = {"fixer": {"status": "RETRY", "ts": "2026-07-23T11:05:00", "phase": "fixer", "model": "m"}}
            rendered = strip_ansi(
                render_worker_detail("ISO", calls, Path(tmpdir), "fixer", 100, now_fn=lambda: 0)
            )
        self.assertIn("Request file missing", rendered)

    def test_missing_response_shows_placeholder(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "2026-07-23T11:00:00-fixer-request.json").write_text(
                json.dumps({"messages": [{"role": "user", "content": "x"}]})
            )
            calls = {"fixer": {"status": "RETRY", "ts": "2026-07-23T11:00:00", "phase": "fixer", "model": "m"}}
            rendered = strip_ansi(
                render_worker_detail("JPEG", calls, req_log_dir, "fixer", 100, now_fn=lambda: 0)
            )
        self.assertIn("no response yet", rendered)


class ClampIndexTests(unittest.TestCase):
    def test_within_range_unchanged(self):
        self.assertEqual(clamp_index(2, 5), 2)

    def test_negative_clamps_to_zero(self):
        self.assertEqual(clamp_index(-1, 5), 0)

    def test_too_large_clamps_to_last(self):
        self.assertEqual(clamp_index(99, 5), 4)

    def test_empty_count_returns_zero(self):
        self.assertEqual(clamp_index(0, 0), 0)
        self.assertEqual(clamp_index(-3, 0), 0)


class ComputeMaxScrollTests(unittest.TestCase):
    def test_content_taller_than_viewport(self):
        self.assertEqual(compute_max_scroll(100, 20), 80)

    def test_content_shorter_than_viewport_is_zero(self):
        self.assertEqual(compute_max_scroll(5, 20), 0)

    def test_exact_fit_is_zero(self):
        self.assertEqual(compute_max_scroll(20, 20), 0)


class HandleNavigationKeyTests(unittest.TestCase):
    def make_state(self, worker_index=1, scroll_offset=5, phase="fixer"):
        return {"worker_index": worker_index, "scroll_offset": scroll_offset, "phase": phase}

    def test_quit_key_signals_quit_without_changing_state(self):
        state = self.make_state()
        new_state, should_quit = handle_navigation_key(
            FAKE_KEY_CODES["quit"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertTrue(should_quit)
        self.assertIs(new_state, state)

    def test_right_advances_worker_and_resets_scroll(self):
        state = self.make_state(worker_index=0, scroll_offset=7)
        new_state, should_quit = handle_navigation_key(
            FAKE_KEY_CODES["right"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertFalse(should_quit)
        self.assertEqual(new_state["worker_index"], 1)
        self.assertEqual(new_state["scroll_offset"], 0)

    def test_right_wraps_at_upper_bound_via_clamp(self):
        state = self.make_state(worker_index=2, scroll_offset=0)
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["right"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state["worker_index"], 2)  # clamped, not wrapped past last

    def test_left_retreats_worker_and_resets_scroll(self):
        state = self.make_state(worker_index=2, scroll_offset=9)
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["left"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state["worker_index"], 1)
        self.assertEqual(new_state["scroll_offset"], 0)

    def test_left_clamps_at_lower_bound(self):
        state = self.make_state(worker_index=0, scroll_offset=0)
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["left"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state["worker_index"], 0)

    def test_down_and_vim_down_increment_scroll(self):
        state = self.make_state(scroll_offset=2)
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["down"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state["scroll_offset"], 3)

        state2 = self.make_state(scroll_offset=2)
        new_state2, _ = handle_navigation_key(
            FAKE_KEY_CODES["vim_down"], state2, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state2["scroll_offset"], 3)

    def test_up_and_vim_up_decrement_scroll_not_below_zero(self):
        state = self.make_state(scroll_offset=2)
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["up"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state["scroll_offset"], 1)

        state2 = self.make_state(scroll_offset=0)
        new_state2, _ = handle_navigation_key(
            FAKE_KEY_CODES["vim_up"], state2, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state2["scroll_offset"], 0)

    def test_page_down_advances_by_page_size(self):
        state = self.make_state(scroll_offset=2)
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["page_down"], state, FAKE_KEY_CODES, worker_count=3, page_size=10
        )
        self.assertEqual(new_state["scroll_offset"], 12)

    def test_page_up_retreats_by_page_size_not_below_zero(self):
        state = self.make_state(scroll_offset=5)
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["page_up"], state, FAKE_KEY_CODES, worker_count=3, page_size=10
        )
        self.assertEqual(new_state["scroll_offset"], 0)

    def test_toggle_phase_flips_and_resets_scroll(self):
        state = self.make_state(scroll_offset=4, phase="fixer")
        new_state, _ = handle_navigation_key(
            FAKE_KEY_CODES["toggle_phase"], state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state["phase"], "reviewer")
        self.assertEqual(new_state["scroll_offset"], 0)

        state2 = self.make_state(scroll_offset=4, phase="reviewer")
        new_state2, _ = handle_navigation_key(
            FAKE_KEY_CODES["toggle_phase"], state2, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertEqual(new_state2["phase"], "fixer")

    def test_unknown_key_leaves_state_unchanged(self):
        state = self.make_state()
        new_state, should_quit = handle_navigation_key(
            999, state, FAKE_KEY_CODES, worker_count=3, page_size=5
        )
        self.assertFalse(should_quit)
        self.assertEqual(new_state, state)


class RenderInteractiveFrameTests(unittest.TestCase):
    def test_empty_workers_shows_placeholder(self):
        state = {"worker_index": 0, "scroll_offset": 0, "phase": "fixer"}
        lines, max_scroll = render_interactive_frame(state, {}, Path("/nonexistent-req-log-dir"), width=80, height=24)
        self.assertEqual(lines, ["No model calls logged yet."])
        self.assertEqual(max_scroll, 0)

    def test_header_shows_worker_position_and_phase(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            latest = {
                "JPEG": {"fixer": {"status": "OK", "ts": "2026-07-23T11:00:00", "phase": "fixer", "model": "m"}},
                "RW2": {"fixer": None},
            }
            state = {"worker_index": 0, "scroll_offset": 0, "phase": "fixer"}
            lines, _ = render_interactive_frame(
                state, latest, req_log_dir, width=80, height=24, now_fn=lambda: 0
            )
        self.assertIn("[1/2] JPEG (fixer)", lines[0])
        self.assertIn("q quit", lines[0])

    def test_worker_index_out_of_range_is_clamped(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            latest = {"JPEG": {"fixer": None}}
            state = {"worker_index": 99, "scroll_offset": 0, "phase": "fixer"}
            lines, _ = render_interactive_frame(
                state, latest, req_log_dir, width=80, height=24, now_fn=lambda: 0
            )
        self.assertIn("[1/1] JPEG", lines[0])

    def test_scroll_offset_clamped_to_max_scroll(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "2026-07-23T11:00:00-fixer-request.json").write_text(
                json.dumps({"messages": [{"role": "user", "content": "line\n" * 200}]})
            )
            (req_log_dir / "2026-07-23T11:00:00-fixer-response.txt").write_text("resp")
            latest = {
                "JPEG": {"fixer": {"status": "OK", "ts": "2026-07-23T11:00:00", "phase": "fixer", "model": "m"}},
            }
            state = {"worker_index": 0, "scroll_offset": 999999, "phase": "fixer"}
            lines, max_scroll = render_interactive_frame(
                state, latest, req_log_dir, width=80, height=10, now_fn=lambda: 0
            )
        self.assertGreater(max_scroll, 0)
        # header + visible body, never exceeding the requested height
        self.assertLessEqual(len(lines), 10)


class MainTests(unittest.TestCase):
    def test_once_mode_renders_overview_and_exits(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "manifest.log").write_text(
                "2026-07-23T11:00:00 phase=fixer worker=A model=m prompt_chars=1 elapsed=1.0s reply_chars=1 OK\n"
            )
            out = io.StringIO()
            rc = main(["--req-log-dir", str(req_log_dir), "--once"], stdout=out)
        self.assertEqual(rc, 0)
        self.assertIn("A", strip_ansi(out.getvalue()))

    def test_once_mode_worker_detail(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            req_log_dir = Path(tmpdir)
            (req_log_dir / "manifest.log").write_text(
                "2026-07-23T11:00:00 phase=fixer worker=A model=m prompt_chars=1 elapsed=1.0s reply_chars=1 OK\n"
            )
            (req_log_dir / "2026-07-23T11:00:00-fixer-request.json").write_text(
                json.dumps({"messages": [{"role": "user", "content": "hello worker"}]})
            )
            (req_log_dir / "2026-07-23T11:00:00-fixer-response.txt").write_text("response text")
            out = io.StringIO()
            rc = main(["--req-log-dir", str(req_log_dir), "--worker", "A", "--once"], stdout=out)
        self.assertEqual(rc, 0)
        rendered = strip_ansi(out.getvalue())
        self.assertIn("hello worker", rendered)
        self.assertIn("response text", rendered)


if __name__ == "__main__":
    unittest.main()
