import io
import json
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from fleet_failrate import (
    classify_ok_absence,
    count_unsettled,
    local_cutoff,
    main,
    read_model_calls,
    read_patch_applies,
    render,
)

OK_LINE = ("{ts} phase=fixer worker={w} tier=T1 model=gpt-5.6-sol "
           "prompt_chars=100 elapsed=1.5s reply_chars=42 OK")
ERR_LINE = ("{ts} phase=fixer worker={w} tier=T1 model=gpt-5.6-sol "
            "prompt_chars=100 elapsed=2.5s ERROR={msg}")
RETRY_LINE = ("{ts} phase=fixer worker={w} tier=T1 model=gpt-5.6-sol "
              "RETRY model call retry 1/1000 after RuntimeError('empty'), waiting 2s")


def ok(ts, w="JPEG"):
    return OK_LINE.format(ts=ts, w=w)


def err(ts, w="JPEG", msg="HTTP Error 502: Bad Gateway"):
    return ERR_LINE.format(ts=ts, w=w, msg=msg)


def retry(ts, w="JPEG"):
    return RETRY_LINE.format(ts=ts, w=w)


class TempLogs:
    """A logs/ tree shaped like ~/.oxidex/logs."""

    def __init__(self, manifest_lines=(), diff_lines=(), request_names=()):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        req = self.root / "model-fix-requests"
        req.mkdir(parents=True)
        (req / "manifest.log").write_text("\n".join(manifest_lines) + ("\n" if manifest_lines else ""))
        diffs = self.root / "model-fix-diffs"
        diffs.mkdir(parents=True)
        (diffs / "manifest.log").write_text("\n".join(diff_lines) + ("\n" if diff_lines else ""))
        for name in request_names:
            (req / name).write_text("{}")

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.tmp.cleanup()

    @property
    def manifest(self):
        return self.root / "model-fix-requests" / "manifest.log"

    @property
    def req_dir(self):
        return self.root / "model-fix-requests"

    @property
    def diff_manifest(self):
        return self.root / "model-fix-diffs" / "manifest.log"


class TestOutcomeCounting(unittest.TestCase):
    def test_ok_error_and_retry_are_counted_separately(self):
        lines = [ok("2026-07-30T16:10:00"), ok("2026-07-30T16:11:00"),
                 err("2026-07-30T16:12:00"), retry("2026-07-30T16:13:00")]
        with TempLogs(lines) as logs:
            counts, existed = read_model_calls(logs.manifest, "")
        self.assertTrue(existed)
        self.assertEqual((counts.ok, counts.err, counts.retry), (2, 1, 1))
        self.assertEqual(counts.settled, 3, "RETRY must not enter the denominator")
        self.assertAlmostEqual(counts.rate, 100.0 / 3)

    def test_retry_is_never_a_failure(self):
        """A call that retried twice and then returned is one success."""
        lines = [retry("2026-07-30T16:10:00"), retry("2026-07-30T16:10:02"),
                 ok("2026-07-30T16:10:00")]
        with TempLogs(lines) as logs:
            counts, _ = read_model_calls(logs.manifest, "")
        self.assertEqual(counts.err, 0)
        self.assertEqual(counts.rate, 0.0)

    def test_window_cutoff_excludes_earlier_lines(self):
        lines = [err("2026-07-30T09:00:00"), ok("2026-07-30T16:10:00")]
        with TempLogs(lines) as logs:
            counts, _ = read_model_calls(logs.manifest, "2026-07-30T16:05")
        self.assertEqual((counts.ok, counts.err), (1, 0))
        self.assertEqual(counts.ok_outside_window, 0)

    def test_error_kinds_are_grouped(self):
        lines = [ok("2026-07-30T16:10:00"),
                 err("2026-07-30T16:11:00", msg="HTTP Error 429: Too Many Requests"),
                 err("2026-07-30T16:12:00", msg="HTTP Error 429: Too Many Requests"),
                 err("2026-07-30T16:13:00", msg="model returned an empty reply")]
        with TempLogs(lines) as logs:
            counts, _ = read_model_calls(logs.manifest, "")
        self.assertEqual(counts.errkinds["HTTP Error 429: Too Many Requests"], 2)
        self.assertEqual(counts.errkinds["model returned an empty reply"], 1)


class TestBlindParserDetection(unittest.TestCase):
    """The defect class this tool exists to prevent: a monitor that cannot
    tell "everything failed" from "I can no longer read the log"."""

    def test_no_ok_anywhere_is_reported_as_blind_not_as_100_percent(self):
        with TempLogs([err("2026-07-30T16:10:00"), err("2026-07-30T16:11:00")]) as logs:
            counts, existed = read_model_calls(logs.manifest, "")
            status, message = classify_ok_absence(counts, existed)
        self.assertEqual(status, "BLIND")
        self.assertIn("no OK records", message)

    def test_no_ok_in_window_but_ok_outside_is_a_real_outage(self):
        lines = [ok("2026-07-30T09:00:00"), err("2026-07-30T16:10:00"),
                 err("2026-07-30T16:11:00")]
        with TempLogs(lines) as logs:
            counts, existed = read_model_calls(logs.manifest, "2026-07-30T16:05")
            status, message = classify_ok_absence(counts, existed)
        self.assertEqual(status, "OUTAGE")
        self.assertIn("real outage", message)

    def test_unparseable_lines_are_blind_not_failures(self):
        """The exact 2026-07-30 regression: a log full of failure-shaped text
        with no success term must never yield a rate."""
        lines = ["2026-07-30T16:10:00 [dispatcher] Traceback (most recent call last)",
                 "2026-07-30T16:11:00 [dispatcher] error[E0432]: unresolved import",
                 "2026-07-30T16:12:00 [dispatcher] review REJECTED: nope"]
        with TempLogs(lines) as logs:
            counts, existed = read_model_calls(logs.manifest, "")
            status, message = classify_ok_absence(counts, existed)
        self.assertEqual(counts.settled, 0)
        self.assertIsNone(counts.rate)
        self.assertEqual(status, "BLIND")
        self.assertIn("understood 0 of 3 lines", message)

    def test_missing_manifest_is_no_data(self):
        with TempLogs() as logs:
            counts, existed = read_model_calls(logs.root / "nope" / "manifest.log", False)
            status, _ = classify_ok_absence(counts, existed)
        self.assertEqual(status, "NO_DATA")

    def test_empty_window_is_no_data(self):
        with TempLogs([ok("2026-07-30T09:00:00")]) as logs:
            counts, existed = read_model_calls(logs.manifest, "2026-07-30T16:05")
            status, _ = classify_ok_absence(counts, existed)
        self.assertEqual(status, "NO_DATA")

    def test_render_withholds_a_rate_when_there_are_no_successes(self):
        with TempLogs([err("2026-07-30T16:10:00")]) as logs:
            counts, _ = read_model_calls(logs.manifest, "")
        lines, ok_to_report = render(counts)
        self.assertFalse(ok_to_report)
        self.assertFalse(any("FAILURE RATE" in line for line in lines),
                         "a rate with no success term must not be printed")


class TestFormatDrift(unittest.TestCase):
    def test_a_new_mid_line_field_does_not_zero_the_success_term(self):
        """`tier=` was added mid-line once already. The next such addition
        must degrade to a warning, never to a fabricated 100%."""
        lines = [
            "2026-07-30T16:10:00 phase=fixer worker=JPEG tier=T1 squad=raw "
            "model=gpt-5.6-sol prompt_chars=100 elapsed=1.5s reply_chars=42 "
            "cache_hit=True OK",
            "2026-07-30T16:11:00 phase=fixer worker=JPEG tier=T1 squad=raw "
            "model=gpt-5.6-sol prompt_chars=100 elapsed=2.5s cache_hit=False "
            "ERROR=HTTP Error 502: Bad Gateway",
        ]
        with TempLogs(lines) as logs:
            counts, _ = read_model_calls(logs.manifest, "")
        self.assertEqual((counts.ok, counts.err), (1, 1))
        self.assertEqual(counts.rate, 50.0)
        self.assertEqual(counts.drift, 2, "drift must be visible, not silent")
        rendered, ok_to_report = render(counts)
        self.assertTrue(ok_to_report)
        self.assertTrue(any("drifting" in line for line in rendered))

    def test_no_drift_warning_for_retry_lines(self):
        lines = [ok("2026-07-30T16:10:00"), retry("2026-07-30T16:10:02")]
        with TempLogs(lines) as logs:
            counts, _ = read_model_calls(logs.manifest, "")
        self.assertEqual(counts.drift, 0)


class TestUnsettledRequests(unittest.TestCase):
    def test_inflight_requests_are_not_failures(self):
        now = time.mktime(time.strptime("2026-07-30T16:20:00", "%Y-%m-%dT%H:%M:%S"))
        with TempLogs(
            [ok("2026-07-30T16:10:00")],
            request_names=["2026-07-30T16:10:00-JPEG-fixer-request.json",
                           "2026-07-30T16:19:00-JPEG-fixer-request.json"],
        ) as logs:
            inflight, stale = count_unsettled(logs.req_dir, logs.manifest, "", 1800, now=now)
            counts, _ = read_model_calls(logs.manifest, "")
        self.assertEqual((inflight, stale), (1, 0))
        self.assertEqual(counts.err, 0, "an in-flight call is not an error")

    def test_old_unsettled_requests_are_stale_not_inflight(self):
        now = time.mktime(time.strptime("2026-07-30T23:00:00", "%Y-%m-%dT%H:%M:%S"))
        with TempLogs(
            [ok("2026-07-30T16:10:00")],
            request_names=["2026-07-30T16:10:00-JPEG-fixer-request.json",
                           "2026-07-30T16:11:00-JPEG-fixer-request.json"],
        ) as logs:
            inflight, stale = count_unsettled(logs.req_dir, logs.manifest, "", 1800, now=now)
        self.assertEqual((inflight, stale), (0, 1))

    def test_worker_labels_containing_hyphens_are_paired(self):
        now = time.mktime(time.strptime("2026-07-30T16:20:00", "%Y-%m-%dT%H:%M:%S"))
        with TempLogs(
            [ok("2026-07-30T16:10:00", w="canon-1")],
            request_names=["2026-07-30T16:10:00-canon-1-fixer-request.json"],
        ) as logs:
            inflight, stale = count_unsettled(logs.req_dir, logs.manifest, "", 1800, now=now)
        self.assertEqual((inflight, stale), (0, 0))

    def test_errored_requests_are_settled_not_inflight(self):
        now = time.mktime(time.strptime("2026-07-30T16:20:00", "%Y-%m-%dT%H:%M:%S"))
        with TempLogs(
            [err("2026-07-30T16:10:00")],
            request_names=["2026-07-30T16:10:00-JPEG-fixer-request.json"],
        ) as logs:
            inflight, stale = count_unsettled(logs.req_dir, logs.manifest, "", 1800, now=now)
        self.assertEqual((inflight, stale), (0, 0))


class TestLocalTimeWindows(unittest.TestCase):
    """Manifests are stamped in local time and the window filter is a string
    prefix compare, so a UTC-derived cutoff lands hours off and matches
    nothing while the fleet is healthy."""

    def test_cutoff_is_local_time_not_utc(self):
        now = time.time()
        cutoff = local_cutoff("30m", now=now)
        expected = time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(now - 1800))
        self.assertEqual(cutoff, expected)
        if time.timezone != 0 or time.daylight:
            utc_cutoff = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(now - 1800))
            if utc_cutoff != expected:
                self.assertNotEqual(cutoff, utc_cutoff)

    def test_duration_units(self):
        now = time.mktime(time.strptime("2026-07-30T12:00:00", "%Y-%m-%dT%H:%M:%S"))
        self.assertEqual(local_cutoff("90s", now=now), "2026-07-30T11:58:30")
        self.assertEqual(local_cutoff("30m", now=now), "2026-07-30T11:30:00")
        self.assertEqual(local_cutoff("2h", now=now), "2026-07-30T10:00:00")
        self.assertEqual(local_cutoff("30", now=now), "2026-07-30T11:30:00")

    def test_bad_duration_is_rejected_loudly(self):
        with self.assertRaises(ValueError):
            local_cutoff("half an hour")

    def test_last_flag_finds_recent_calls(self):
        recent = time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(time.time() - 60))
        diffs = ["{ts} worker=X3F applied=True rung=exact file=a.diff "
                 "apply_msg='applied'".format(ts=recent)]
        with TempLogs([ok(recent)], diff_lines=diffs) as logs:
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main(["--logs", str(logs.root), "--last", "30m"])
        self.assertEqual(rc, 0)
        self.assertIn("FAILURE RATE 0/1 = 0.0%", buf.getvalue())

    def test_empty_window_reports_log_freshness_so_a_bad_window_is_obvious(self):
        """A window in the future matches nothing. The diagnostic must carry
        the last record's own timestamp, so the reader sees at once that the
        log is current and the QUERY is what is wrong."""
        with TempLogs([ok("2026-07-30T16:10:00")]) as logs:
            counts, existed = read_model_calls(logs.manifest, "2099-01-01T00:00")
            status, message = classify_ok_absence(counts, existed)
        self.assertEqual(status, "NO_DATA")
        self.assertIn("2026-07-30T16:10:00", message)
        self.assertIn("now ", message)

    def test_blind_diagnostic_also_reports_freshness(self):
        with TempLogs(["2026-07-30T16:10:00 [dispatcher] Traceback"]) as logs:
            counts, existed = read_model_calls(logs.manifest, "")
            _, message = classify_ok_absence(counts, existed)
        self.assertIn("2026-07-30T16:10:00", message)


class TestPatchApplies(unittest.TestCase):
    def test_applied_true_is_the_success_term(self):
        lines = [
            "2026-07-30T16:10:00 worker=X3F applied=True rung=exact "
            "file=a.diff apply_msg='applied'",
            "2026-07-30T16:11:00 worker=OLE applied=False rung=None "
            "file=b.diff apply_msg='error: patch failed'",
        ]
        with TempLogs(diff_lines=lines) as logs:
            counts, existed = read_patch_applies(logs.diff_manifest, "")
        self.assertTrue(existed)
        self.assertEqual((counts.ok, counts.err), (1, 1))
        self.assertEqual(counts.rate, 50.0)

    def test_legacy_lines_without_worker_are_still_counted(self):
        """358 real lines predate worker-tagging. A first cut of this parser
        required `worker=` and silently dropped every one -- the same
        over-strict-parser mistake the tool exists to catch."""
        lines = [
            "2026-07-22T19:37:32 applied=True file=a.diff apply_msg='applied'",
            "2026-07-22T19:45:17 applied=False file=b.diff "
            "apply_msg='error: patch with only garbage at line 4\\n'",
            "2026-07-30T16:10:00 worker=X3F applied=True rung=exact "
            "file=c.diff apply_msg='applied'",
        ]
        with TempLogs(diff_lines=lines) as logs:
            counts, _ = read_patch_applies(logs.diff_manifest, "")
        self.assertEqual((counts.ok, counts.err), (2, 1))
        self.assertEqual(counts.ok + counts.err, len(lines),
                         "every line in the file must be accounted for")
        self.assertEqual(counts.drift, 0)

    def test_unknown_diff_shape_shows_as_drift_not_as_a_dropped_record(self):
        lines = [
            "2026-07-30T16:10:00 worker=X3F squad=raw applied=True "
            "rung=exact file=a.diff apply_msg='applied' cached=True",
            "2026-07-30T16:11:00 worker=OLE squad=raw applied=False "
            "rung=None file=b.diff apply_msg='nope' cached=False",
        ]
        with TempLogs(diff_lines=lines) as logs:
            counts, _ = read_patch_applies(logs.diff_manifest, "")
        self.assertEqual((counts.ok, counts.err), (1, 1))
        self.assertEqual(counts.drift, 2)

    def test_all_rejected_with_no_prior_success_is_blind(self):
        lines = ["2026-07-30T16:11:00 worker=OLE applied=False rung=None "
                 "file=b.diff apply_msg='error'"]
        with TempLogs(diff_lines=lines) as logs:
            counts, existed = read_patch_applies(logs.diff_manifest, "")
            status, _ = classify_ok_absence(counts, existed)
        self.assertEqual(status, "BLIND")


class TestMain(unittest.TestCase):
    def test_healthy_fleet_prints_a_rate_and_exits_zero(self):
        lines = [ok(f"2026-07-30T16:1{i}:00") for i in range(9)] + [err("2026-07-30T16:19:00")]
        diffs = ["2026-07-30T16:10:00 worker=X3F applied=True rung=exact "
                 "file=a.diff apply_msg='applied'"]
        with TempLogs(lines, diff_lines=diffs) as logs:
            out = io.StringIO()
            with redirect_stdout(out):
                rc = main(["--logs", str(logs.root)])
        self.assertEqual(rc, 0)
        self.assertIn("FAILURE RATE 1/10 = 10.0%", out.getvalue())

    def test_blind_parser_exits_two_and_never_prints_a_rate(self):
        lines = ["2026-07-30T16:10:00 [dispatcher] Traceback (most recent call last)"]
        with TempLogs(lines) as logs:
            out = io.StringIO()
            with redirect_stdout(out):
                rc = main(["--logs", str(logs.root)])
        rendered = out.getvalue()
        self.assertEqual(rc, 2, "an unreadable log must be as loud as a broken fleet")
        self.assertNotIn("100.0%", rendered)
        self.assertIn("BLIND", rendered)
        self.assertIn("format may have changed", rendered)

    def test_positional_cutoff_matches_since_flag(self):
        lines = [err("2026-07-30T09:00:00"), ok("2026-07-30T16:10:00")]
        diffs = ["2026-07-30T16:10:00 worker=X3F applied=True rung=exact "
                 "file=a.diff apply_msg='applied'"]
        with TempLogs(lines, diff_lines=diffs) as logs:
            outs = []
            for argv in (["--logs", str(logs.root), "2026-07-30T16:05"],
                         ["--logs", str(logs.root), "--since", "2026-07-30T16:05"]):
                buf = io.StringIO()
                with redirect_stdout(buf):
                    main(argv)
                outs.append(buf.getvalue())
        self.assertEqual(outs[0], outs[1])
        self.assertIn("FAILURE RATE 0/1 = 0.0%", outs[0])

    def test_json_output_carries_the_problem_list(self):
        with TempLogs(["2026-07-30T16:10:00 nonsense"]) as logs:
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = main(["--logs", str(logs.root), "--json"])
        payload = json.loads(buf.getvalue())
        self.assertEqual(rc, 2)
        self.assertIsNone(payload["model_calls"]["rate_pct"])
        self.assertTrue(payload["problems"])
        self.assertEqual(payload["problems"][0]["status"], "BLIND")


if __name__ == "__main__":
    unittest.main()
