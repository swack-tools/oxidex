import json
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch, MagicMock

import find_tag_gaps
from find_tag_gaps import (
    build_semaphore,
    ensure_tag_comparison_built,
    group_gaps_by_format,
    locate_parser_files,
    run_full_comparison,
    run_format_comparison,
)

FIXTURE = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "comparison_report_sample.json"


class GroupGapsByFormatTests(unittest.TestCase):
    def setUp(self):
        with open(FIXTURE) as f:
            self.report = json.load(f)

    def test_sorts_by_gap_count_descending(self):
        gaps = group_gaps_by_format(self.report)
        counts = [g["gap_count"] for g in gaps]
        self.assertEqual(counts, sorted(counts, reverse=True))

    def test_skips_formats_with_no_gaps(self):
        gaps = group_gaps_by_format(self.report)
        formats = {g["format"] for g in gaps}
        self.assertNotIn("PNG", formats)

    def test_gap_count_is_missing_plus_differences(self):
        gaps = group_gaps_by_format(self.report)
        nef = next(g for g in gaps if g["format"] == "NEF")
        self.assertEqual(nef["gap_count"], len(nef["missing_tags"]) + len(nef["value_differences"]))
        self.assertEqual(nef["gap_count"], 4)

    def test_includes_missing_tags_and_value_differences_verbatim(self):
        gaps = group_gaps_by_format(self.report)
        jpeg = next(g for g in gaps if g["format"] == "JPEG")
        self.assertEqual(jpeg["missing_tags"][0]["name"], "LensModel")
        self.assertEqual(jpeg["value_differences"][0]["tag_key"], "EXIF:ISO")


class DuplicateEmissionVisibilityTests(unittest.TestCase):
    """A format at 100% parity that still emits one tag twice must stay
    visible to the publish gates (2026-07-26).

    This is the GIF.gif shape: BackgroundColor was emitted both bare and
    as GIF:BackgroundColor, both normalize to GIF:BackgroundColor, and
    which value survived was a per-process coin flip -- so the format
    alternated between "one value difference" and "35/35, nothing to
    report" on an unchanged tree. Once the winner is deterministic and
    happens to match ExifTool, gap_count is 0 and the old
    `if gap_count == 0: continue` dropped the format from the list
    entirely, taking its duplicate_emissions with it.
    """

    def _report(self, duplicates):
        return {"by_format": {"GIF": {
            "missing_in_oxidex": [], "value_differences": [],
            "duplicate_emissions": duplicates,
        }}}

    def test_zero_gap_format_with_duplicate_emissions_is_still_listed(self):
        gaps = group_gaps_by_format(self._report(["GIF:BackgroundColor"]))
        self.assertEqual([g["format"] for g in gaps], ["GIF"])
        self.assertEqual(gaps[0]["duplicate_emissions"], ["GIF:BackgroundColor"])
        self.assertEqual(gaps[0]["gap_count"], 0)

    def test_zero_gap_format_without_duplicates_is_still_skipped(self):
        self.assertEqual(group_gaps_by_format(self._report([])), [])

    def test_publish_gate_lookup_sees_the_duplicate(self):
        """The exact shape squad_merge_loop.real_format_match uses, and
        the exact read process_commit / run_batch_check /
        evaluate_post_merge then perform. Before this change the lookup
        returned None and every one of them read `[]` -- a silent pass.
        """
        gaps = group_gaps_by_format(self._report(["GIF:BackgroundColor"]))
        match = next((g for g in gaps if g["format"] == "GIF"), None)
        self.assertIsNotNone(match, "real_format_match would have returned None")
        self.assertEqual((match or {}).get("duplicate_emissions") or [],
                         ["GIF:BackgroundColor"])

    def test_duplicate_only_entry_dispatches_no_work(self):
        """Surfacing a zero-gap format must not put a worker to work on a
        format with nothing to fix: run_tag_loop's unit of work comes from
        expand_gaps_to_tags, which reads only missing_tags and
        value_differences.
        """
        from model_fix_loop import expand_gaps_to_tags
        gaps = group_gaps_by_format(self._report(["GIF:BackgroundColor"]))
        self.assertEqual(expand_gaps_to_tags(gaps), [])

    def test_duplicate_only_entry_sorts_after_real_gaps(self):
        report = {"by_format": {
            "GIF": {"missing_in_oxidex": [], "value_differences": [],
                    "duplicate_emissions": ["GIF:BackgroundColor"]},
            "NEF": {"missing_in_oxidex": [{"family": "EXIF", "name": "LensModel"}],
                    "value_differences": [], "duplicate_emissions": []},
        }}
        self.assertEqual([g["format"] for g in group_gaps_by_format(report)], ["NEF", "GIF"])


class LocateParserFilesTests(unittest.TestCase):
    def test_jpeg_maps_to_a_real_directory(self):
        files = locate_parser_files("JPEG")
        self.assertTrue(any("src/parsers/jpeg" in f or "src/core" in f for f in files))
        self.assertGreater(len(files), 0)

    def test_unknown_format_with_no_matching_directory_returns_empty(self):
        files = locate_parser_files("TotallyMadeUpFormat")
        self.assertEqual(files, [])


# The /tmp/... literals below are inert test-fixture values passed to a
# mocked subprocess.run -- no real filesystem I/O happens in this file.
class RunFullComparisonTests(unittest.TestCase):
    @patch("find_tag_gaps.subprocess.run")
    def test_invokes_just_with_cache_dir_env(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        result = run_full_comparison("/tmp/fake-cache", repo_root=Path("/fake/repo"))  # nosec B108
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["just", "compare-exiftool-full"])
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))
        self.assertEqual(kwargs["env"]["EXIFTOOL_CACHE_DIR"], "/tmp/fake-cache")  # nosec B108
        self.assertEqual(result, Path("/fake/repo/comparison.json"))


class RunFormatComparisonTests(unittest.TestCase):
    @patch("find_tag_gaps.ensure_tag_comparison_built")
    @patch("find_tag_gaps.subprocess.run")
    def test_invokes_tag_comparison_with_format_flag(self, mock_run, mock_ensure):
        mock_run.return_value = MagicMock(returncode=0)
        result = run_format_comparison("NEF", "/tmp/fake-cache", repo_root=Path("/fake/repo"))  # nosec B108
        mock_ensure.assert_called_once_with(Path("/fake/repo"), semaphore_path=None, semaphore_max_holders=5)
        args, kwargs = mock_run.call_args
        self.assertIn("--format", args[0])
        self.assertIn("NEF", args[0])
        self.assertIn("--samples", args[0])
        self.assertIn("/tmp/fake-cache/combined-samples", args[0])  # nosec B108
        self.assertEqual(result, Path("/tmp/tagcmp-NEF.json"))  # nosec B108

    @patch("find_tag_gaps.ensure_tag_comparison_built")
    @patch("find_tag_gaps.subprocess.run")
    def test_empty_out_suffix_keeps_the_legacy_paths(self, mock_run, mock_ensure):
        mock_run.return_value = MagicMock(returncode=0)
        result = run_format_comparison(
            "JPEG", "/tmp/fake-cache", repo_root=Path("/fake/repo"), out_suffix="",  # nosec B108
        )
        self.assertEqual(result, Path("/tmp/tagcmp-JPEG.json"))  # nosec B108
        args, _ = mock_run.call_args
        self.assertIn("/tmp/tagcmp-JPEG-md", args[0])  # nosec B108

    @patch("find_tag_gaps.ensure_tag_comparison_built")
    @patch("find_tag_gaps.subprocess.run")
    def test_out_suffix_gives_each_worker_its_own_output_paths(self, mock_run, mock_ensure):
        # Two workers re-checking the same format must write disjoint
        # report and markdown paths -- the shared fixed /tmp path let
        # them overwrite each other's report mid-recheck.
        mock_run.return_value = MagicMock(returncode=0)
        result_1 = run_format_comparison(
            "JPEG", "/tmp/fake-cache", repo_root=Path("/fake/repo"), out_suffix="JPEG-1",  # nosec B108
        )
        argv_1 = mock_run.call_args.args[0]
        result_2 = run_format_comparison(
            "JPEG", "/tmp/fake-cache", repo_root=Path("/fake/repo"), out_suffix="JPEG-2",  # nosec B108
        )
        argv_2 = mock_run.call_args.args[0]

        self.assertEqual(result_1, Path("/tmp/tagcmp-JPEG-JPEG-1.json"))  # nosec B108
        self.assertEqual(result_2, Path("/tmp/tagcmp-JPEG-JPEG-2.json"))  # nosec B108
        self.assertNotEqual(result_1, result_2)
        self.assertIn("/tmp/tagcmp-JPEG-JPEG-1-md", argv_1)  # nosec B108
        self.assertIn("/tmp/tagcmp-JPEG-JPEG-2-md", argv_2)  # nosec B108


class BuildSemaphoreTests(unittest.TestCase):
    """Spec section 5's build semaphore -- a cross-process flock-based
    counting semaphore (N holders max), the simpler twin of
    model_fix_loop.py's _governor_locked (no token bucket, just a
    holder-count ceiling with per-holder heartbeats for stale-holder
    recovery)."""

    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.path = Path(self.tmpdir.name) / "build-semaphore.json"

    def test_path_none_is_a_noop(self):
        entered = []
        with build_semaphore(None):
            entered.append(1)
        self.assertEqual(entered, [1])

    def test_single_holder_acquires_and_releases(self):
        with build_semaphore(self.path, max_holders=2):
            state = json.loads(self.path.read_text())
            self.assertEqual(len(state["holders"]), 1)
        state = json.loads(self.path.read_text())
        self.assertEqual(state["holders"], {})

    def test_third_holder_blocks_until_a_slot_frees(self):
        # Pre-seed two live holders (simulating two already-running
        # cargo builds) so a third caller must wait.
        now = time.time()
        self.path.write_text(json.dumps({
            "holders": {
                "h1": {"pid": 1, "heartbeat": now},
                "h2": {"pid": 2, "heartbeat": now},
            },
        }))
        waited = []

        def fake_sleep(seconds):
            waited.append(seconds)
            # Free up "h1"'s slot on the first sleep so the loop can
            # proceed rather than spinning forever in the test.
            state = json.loads(self.path.read_text())
            state["holders"].pop("h1", None)
            self.path.write_text(json.dumps(state))

        entered = []
        with build_semaphore(self.path, max_holders=2, sleep_fn=fake_sleep,
                             holder_id="h3", poll_seconds=0):
            entered.append(1)
        self.assertEqual(entered, [1])
        self.assertGreaterEqual(len(waited), 1)

    def test_releases_the_slot_even_when_the_body_raises(self):
        with self.assertRaises(ValueError):
            with build_semaphore(self.path, max_holders=1, holder_id="h1"):
                raise ValueError("boom")
        state = json.loads(self.path.read_text())
        self.assertNotIn("h1", state["holders"])

    def test_stale_holder_is_evicted_and_reused(self):
        stale_time = time.time() - 10_000  # long past any reasonable stale_seconds
        self.path.write_text(json.dumps({"holders": {"h1": {"pid": 1, "heartbeat": stale_time}}}))
        entered = []
        with build_semaphore(self.path, max_holders=1, stale_seconds=900, holder_id="h2"):
            entered.append(1)
        self.assertEqual(entered, [1])

    def test_renewing_the_same_holder_id_never_blocks_itself(self):
        # A nested/re-entrant call with the SAME holder_id (e.g. a
        # heartbeat-style renewal) must not deadlock against its own
        # already-held slot.
        with build_semaphore(self.path, max_holders=1, holder_id="h1"):
            with build_semaphore(self.path, max_holders=1, holder_id="h1"):
                pass  # must not block

    def test_heartbeat_keeps_a_slow_held_slot_alive_past_the_stale_threshold(self):
        # Fake clock: the slot is stamped at t=1000, then the protected
        # call "runs" long enough that the clock reads t=3000 -- 2000
        # fake seconds, past stale_seconds=900, so without a heartbeat
        # any concurrent waiter's _try_acquire_build_slot check would
        # treat this holder as dead and steal its slot. The heartbeat
        # thread (real thread, tiny real cadence, fake timestamps)
        # re-stamps the holder's heartbeat with the advanced clock, so
        # it reads fresh for as long as the slot is held.
        clock = [1000.0]
        observed = {}

        def slow_body():
            clock[0] = 3000.0
            deadline = time.time() + 10
            while time.time() < deadline:
                state = json.loads(self.path.read_text())
                entry = state["holders"].get("h1") or {}
                if entry.get("heartbeat") == 3000.0:
                    observed.update(entry)
                    break
                time.sleep(0.005)

        with build_semaphore(
            self.path, max_holders=1, stale_seconds=900, holder_id="h1",
            heartbeat_seconds=0.01, now_fn=lambda: clock[0],
        ):
            slow_body()

        self.assertEqual(observed.get("heartbeat"), 3000.0)
        # The staleness math a concurrent waiter would run mid-build: the
        # original stamp (t=1000) would read abandoned; the heartbeat's
        # re-stamp does not.
        self.assertGreaterEqual(3000.0 - 1000.0, 900)  # un-heartbeated holder would be stale
        self.assertLess(3000.0 - observed["heartbeat"], 900)  # heartbeated holder is fresh

    def test_heartbeat_thread_stops_when_the_slot_is_released(self):
        # After build_semaphore's `with` block exits, no
        # build-semaphore-heartbeat-* thread may still be running
        # (threading.Event stop + join, mirroring model_fix_loop.py's
        # own claim-heartbeat thread lifecycle).
        with build_semaphore(self.path, max_holders=1, holder_id="h1", heartbeat_seconds=0.01):
            pass
        self.assertFalse(
            [t.name for t in threading.enumerate() if t.name.startswith("build-semaphore-heartbeat-")]
        )

    def test_heartbeat_disabled_when_heartbeat_seconds_is_falsy(self):
        with build_semaphore(self.path, max_holders=1, holder_id="h1", heartbeat_seconds=0):
            self.assertFalse(
                [t.name for t in threading.enumerate() if t.name.startswith("build-semaphore-heartbeat-")]
            )

    def test_heartbeat_survives_a_transient_touch_failure(self):
        # A heartbeat touch can legitimately raise mid-build (ENOSPC/
        # EACCES, a torn read racing another holder's write). One such
        # raise must not kill the daemon thread for the rest of a
        # long-running build: proven end-to-end here by making the
        # FIRST heartbeat touch raise, then observing a later re-stamp
        # at the advanced clock -- a beat that can only have come from
        # the thread surviving its failure.
        clock = [1000.0]
        touch_failed = threading.Event()
        observed = {}
        real_try_acquire = find_tag_gaps._try_acquire_build_slot

        def flaky_try_acquire(*args, **kwargs):
            if (threading.current_thread().name.startswith("build-semaphore-heartbeat-")
                    and not touch_failed.is_set()):
                touch_failed.set()
                raise ValueError("synthetic transient touch failure")
            return real_try_acquire(*args, **kwargs)

        def slow_body():
            clock[0] = 3000.0
            deadline = time.time() + 10
            while time.time() < deadline:
                if touch_failed.is_set():
                    state = json.loads(self.path.read_text())
                    entry = state["holders"].get("h1") or {}
                    if entry.get("heartbeat") == 3000.0:
                        observed.update(entry)
                        break
                time.sleep(0.005)

        with patch("find_tag_gaps._try_acquire_build_slot", side_effect=flaky_try_acquire):
            with build_semaphore(
                self.path, max_holders=1, stale_seconds=900, holder_id="h1",
                heartbeat_seconds=0.01, now_fn=lambda: clock[0],
            ):
                slow_body()

        self.assertTrue(touch_failed.is_set())
        self.assertEqual(observed.get("heartbeat"), 3000.0)


class EnsureTagComparisonBuiltSemaphoreTests(unittest.TestCase):
    @patch("find_tag_gaps.subprocess.run")
    def test_default_semaphore_path_none_never_touches_a_lock_file(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        # No semaphore_path given -- must not create any lock file, and
        # must not raise even though no real filesystem location for a
        # semaphore was ever specified.
        ensure_tag_comparison_built(repo_root=Path("/fake/repo"))
        mock_run.assert_called_once()

    @patch("find_tag_gaps.subprocess.run")
    def test_semaphore_path_given_wraps_the_build_in_a_held_slot(self, mock_run):
        with tempfile.TemporaryDirectory() as tmpdir:
            sem_path = Path(tmpdir) / "sem.json"
            observed_holders_during_build = {}

            def fake_run(*args, **kwargs):
                observed_holders_during_build["holders"] = json.loads(sem_path.read_text())["holders"]
                return MagicMock(returncode=0)

            mock_run.side_effect = fake_run
            ensure_tag_comparison_built(
                repo_root=Path("/fake/repo"), semaphore_path=sem_path, semaphore_max_holders=1,
            )
            self.assertEqual(len(observed_holders_during_build["holders"]), 1)
            # Released after the call.
            self.assertEqual(json.loads(sem_path.read_text())["holders"], {})


if __name__ == "__main__":
    unittest.main()
