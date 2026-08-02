#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = ["pytest"]
# ///
"""Tests for rate_limit_report.py.

The thing under test is a measuring instrument, so the tests are mostly
about what it refuses to do: silently drop lines it cannot parse, invent a
per-class baseline that does not exist, or divide a window it has no data
for. A wrong number here is worse than no number, because the whole point
of the P1 phase is that the previous number was unreadable.
"""

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from rate_limit_report import (  # noqa: E402
    BASELINE_429_TOTAL,
    RPM,
    TERMINAL_CAP,
    WINDOW_CAP,
    count_legacy_429s,
    main,
    observed_days,
    parse_since,
    read_events,
    render,
    summarise,
)

# 2026-08-03T12:00:00 and 2026-08-04T12:00:00 local -- two distinct days.
DAY1 = 1785931200.0
DAY2 = DAY1 + 86400


def event(**kw):
    base = {"ts": DAY1, "role": "fixer", "model": "gpt-5.6-sol",
            "endpoint": "https://gw", "outcome": "ok", "error_class": None,
            "attempt": 0, "latency_s": 1.0}
    base.update(kw)
    return base


class ParseSinceTests(unittest.TestCase):
    def test_relative_units(self):
        now = 1_000_000.0
        self.assertEqual(parse_since("24h", now), now - 86400)
        self.assertEqual(parse_since("72h", now), now - 3 * 86400)
        self.assertEqual(parse_since("7d", now), now - 7 * 86400)
        self.assertEqual(parse_since("30m", now), now - 1800)

    def test_iso_date(self):
        self.assertIsNotNone(parse_since("2026-08-03"))

    def test_none_means_no_bound(self):
        self.assertIsNone(parse_since(None))

    def test_garbage_returns_none_rather_than_raising(self):
        # main() turns this into a non-zero exit. Reporting the wrong
        # window silently would be worse than refusing.
        self.assertIsNone(parse_since("last tuesday"))


class ReadEventsTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(__file__).resolve().parent / "_t_events.jsonl"

    def tearDown(self):
        self.tmp.unlink(missing_ok=True)

    def test_missing_file_is_reported_not_guessed(self):
        events, malformed, exists = read_events(self.tmp / "nope")
        self.assertFalse(exists)
        self.assertEqual(events, [])

    def test_malformed_lines_are_counted_not_silently_skipped(self):
        self.tmp.write_text(
            json.dumps(event()) + "\n"
            + "{not json\n"
            + "[1, 2, 3]\n"          # valid JSON, wrong shape
            + json.dumps(event()) + "\n"
        )
        events, malformed, exists = read_events(self.tmp)
        self.assertTrue(exists)
        self.assertEqual(len(events), 2)
        self.assertEqual(malformed, 2)

    def test_blank_lines_are_not_malformed(self):
        self.tmp.write_text(json.dumps(event()) + "\n\n   \n")
        events, malformed, _ = read_events(self.tmp)
        self.assertEqual((len(events), malformed), (1, 0))

    def test_since_filters_by_timestamp(self):
        self.tmp.write_text(
            json.dumps(event(ts=DAY1)) + "\n" + json.dumps(event(ts=DAY2)) + "\n"
        )
        events, _, _ = read_events(self.tmp, since=DAY2 - 1)
        self.assertEqual([e["ts"] for e in events], [DAY2])

    def test_an_event_with_no_timestamp_is_kept_not_dropped(self):
        # A record the parser cannot place in time is still evidence that a
        # call happened. Dropping it would quietly shrink the denominator.
        self.tmp.write_text(json.dumps(event(ts=None)) + "\n")
        events, malformed, _ = read_events(self.tmp, since=DAY2)
        self.assertEqual((len(events), malformed), (1, 0))


class SummariseTests(unittest.TestCase):
    def test_counts_each_rate_limit_class_separately(self):
        events = [
            event(outcome="error", error_class=RPM),
            event(outcome="error", error_class=RPM),
            event(outcome="error", error_class=WINDOW_CAP),
            event(outcome="error", error_class=TERMINAL_CAP),
            event(outcome="ok"),
        ]
        by_class, by_outcome, _ = summarise(events)
        self.assertEqual(by_class[RPM], 2)
        self.assertEqual(by_class[WINDOW_CAP], 1)
        self.assertEqual(by_class[TERMINAL_CAP], 1)
        self.assertEqual(by_outcome["ok"], 1)
        self.assertEqual(by_outcome["error"], 4)

    def test_non_rate_limit_errors_are_not_counted_as_429s(self):
        events = [
            event(outcome="error", error_class="connection"),
            event(outcome="error", error_class="http_403"),
            event(outcome="error", error_class="deadline"),
        ]
        by_class, _, grouped = summarise(events)
        self.assertEqual(by_class["connection"], 1)
        self.assertEqual(grouped["role"], {})

    def test_groups_rate_limits_by_role_model_endpoint_and_day(self):
        events = [
            event(ts=DAY1, outcome="error", error_class=RPM, role="reviewer"),
            event(ts=DAY2, outcome="error", error_class=RPM, role="reviewer"),
        ]
        _, _, grouped = summarise(events)
        self.assertEqual(grouped["role"][f"reviewer [{RPM}]"], 2)
        self.assertEqual(len(grouped["day"]), 2)

    def test_observed_days_counts_distinct_days_not_events(self):
        events = [event(ts=DAY1), event(ts=DAY1 + 60), event(ts=DAY2)]
        self.assertEqual(observed_days(events), 2)


class LegacyManifestTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(__file__).resolve().parent / "_t_manifest.log"

    def tearDown(self):
        self.tmp.unlink(missing_ok=True)

    def test_counts_429_error_lines_only(self):
        self.tmp.write_text(
            "2026-08-03T10:00:00 phase=fixer model=m OK\n"
            "2026-08-03T10:00:01 phase=fixer model=m ERROR=HTTP Error 429\n"
            "2026-08-03T10:00:02 phase=fixer model=m ERROR=HTTP Error 500\n"
            "2026-08-03T10:00:03 phase=fixer model=m RETRY something 429\n"
        )
        hits, errors, exists = count_legacy_429s(self.tmp)
        self.assertTrue(exists)
        self.assertEqual((hits, errors), (1, 2))

    def test_missing_file_is_reported(self):
        self.assertEqual(count_legacy_429s(self.tmp / "nope")[2], False)


class RenderTests(unittest.TestCase):
    """The report's job is to prevent a wrong conclusion, so these assert
    on what it SAYS, not just what it counts."""

    def _render(self, events, legacy=(0, 0, False), group_by=()):
        lines = []
        by_class, by_outcome, grouped = summarise(events)
        render(by_class, by_outcome, grouped, events, 0, legacy,
               group_by=group_by, out=lines.append)
        return "\n".join(lines)

    def test_it_refuses_to_imply_a_per_class_delta(self):
        text = self._render([event(outcome="error", error_class=RPM)])
        self.assertIn("UNCLASSIFIED", text)
        self.assertIn("Do not report a per-class delta", text)
        self.assertIn(str(BASELINE_429_TOTAL), text)

    def test_empty_window_says_so_instead_of_printing_zeros(self):
        text = self._render([])
        self.assertIn("no classified events", text)

    def test_an_empty_window_with_legacy_data_explains_the_likely_cause(self):
        # Zero classified events plus thousands of legacy 429s almost
        # always means the new worker is not deployed -- not that the
        # rate limits stopped.
        text = self._render([], legacy=(27662, 30000, True))
        self.assertIn("may not be deployed", text)

    def test_rpm_domination_points_at_concurrency_not_retry_tuning(self):
        events = [event(outcome="error", error_class=RPM) for _ in range(10)]
        text = self._render(events)
        self.assertIn("fleet concurrency", text)

    def test_cost_caps_are_called_out_when_present(self):
        text = self._render([event(outcome="error", error_class=TERMINAL_CAP)])
        self.assertIn("Cost caps are present", text)

    def test_an_unknown_class_from_a_newer_worker_is_flagged_as_unknown(self):
        text = self._render([event(outcome="error", error_class="quantum_cap")])
        self.assertIn("quantum_cap", text)
        self.assertIn("newer worker", text)

    def test_known_non_rate_limit_classes_are_not_called_unknown(self):
        # connection/deadline/http_403 are ordinary failures this build
        # knows about. Filing them under "a newer worker wrote them" would
        # send someone looking for a version mismatch that isn't there.
        events = [
            event(outcome="error", error_class="connection"),
            event(outcome="error", error_class="deadline"),
            event(outcome="error", error_class="http_403"),
        ]
        text = self._render(events)
        self.assertIn("not rate limits", text)
        self.assertNotIn("newer worker", text)

    def test_non_rate_limit_errors_are_excluded_from_the_429_total(self):
        events = [
            event(outcome="error", error_class=RPM),
            event(outcome="error", error_class="connection"),
            event(outcome="error", error_class="http_403"),
        ]
        text = self._render(events)
        self.assertIn("TOTAL 429            1", text)

    def test_grouping_is_only_rendered_when_asked_for(self):
        events = [event(outcome="error", error_class=RPM, role="reviewer")]
        self.assertNotIn("BY ROLE", self._render(events))
        self.assertIn("BY ROLE", self._render(events, group_by=("role",)))


class MainTests(unittest.TestCase):
    def setUp(self):
        self.logs = Path(__file__).resolve().parent / "_t_logs"
        self.logs.mkdir(exist_ok=True)

    def tearDown(self):
        for p in sorted(self.logs.rglob("*"), reverse=True):
            p.unlink() if p.is_file() else p.rmdir()
        self.logs.rmdir()

    def test_missing_event_log_exits_nonzero(self):
        self.assertEqual(main(["--logs", str(self.logs)]), 1)

    def test_unparseable_since_exits_nonzero(self):
        (self.logs / "model-calls.jsonl").write_text(json.dumps(event()) + "\n")
        self.assertEqual(main(["--logs", str(self.logs), "--since", "yesterday"]), 2)

    def test_json_output_is_machine_readable_and_flags_the_baseline(self):
        (self.logs / "model-calls.jsonl").write_text(
            json.dumps(event(outcome="error", error_class=RPM)) + "\n"
            + json.dumps(event(outcome="error", error_class=TERMINAL_CAP)) + "\n"
        )
        import contextlib
        import io
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = main(["--logs", str(self.logs), "--since", "all", "--json"])
        self.assertEqual(code, 0)
        payload = json.loads(buf.getvalue())
        self.assertEqual(payload["rate_limits"][RPM], 1)
        self.assertEqual(payload["rate_limits"][TERMINAL_CAP], 1)
        self.assertEqual(payload["rate_limits_total"], 2)
        # The consumer must not be able to mistake the baseline for a breakdown.
        self.assertIs(payload["baseline"]["classified"], False)


if __name__ == "__main__":
    unittest.main()
