import io
import json
import re
import tempfile
import unittest
from pathlib import Path

from watch_context import (
    format_elapsed_ago,
    latest_calls_per_worker,
    load_manifest_entries,
    load_request_messages,
    load_response_text,
    main,
    parse_manifest_line,
    render_call_summary,
    render_overview,
    render_worker_detail,
)

ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def strip_ansi(s):
    return ANSI_RE.sub("", s)


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
        rendered = strip_ansi(render_worker_detail("X", None, Path("/tmp"), "fixer", 100))
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
