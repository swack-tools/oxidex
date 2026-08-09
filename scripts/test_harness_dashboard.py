#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Hermetic checks for harness_dashboard: durable-log parsers and fleet controls.

Every fleet-control test runs against mocks or temp files: no test may invoke
launchctl, start a fleet, or touch ~/Library/LaunchAgents. The real
LaunchAgent plist is only ever read by the live server, never by this suite.
"""

import http.client
import json
import plistlib
import tempfile
import threading
import unittest
from http import HTTPStatus
from pathlib import Path
from unittest import mock

import harness_dashboard as dashboard


def make_plist(path: Path, args: list[str] | None = None, label: str = "com.oxidex.fleet") -> None:
    data = {"Label": label, "ProgramArguments": args if args is not None else ["/bin/bash", "/deploy/scripts/fleet_up.sh", "--workers", "3", "--squad-mode", "--config", "/deploy/config.toml"], "RunAtLoad": True}
    with path.open("wb") as handle:
        plistlib.dump(data, handle)


def completed(returncode: int = 0, stdout: str = "", stderr: str = "") -> mock.Mock:
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


class DashboardParserTests(unittest.TestCase):
    def test_manifest_counts_phases_and_keeps_newest_task(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.log"
            path.write_text(
                "2026-08-04T12:00:00 phase=fixer worker=canon-1 provider=x http_status=200 OK\n"
                "2026-08-04T12:01:00 phase=reviewer worker=canon-1 provider=x http_status=401 ERROR=HTTP\n"
                "2026-08-04T11:00:00 phase=fixer worker=xmp-2 provider=x ERROR=HTTP Error 500\n"
            )
            result = dashboard.manifest_stats(path)
        self.assertEqual(result["canon-1"]["fixer_calls"], 1)
        self.assertEqual(result["canon-1"]["reviewer_calls"], 1)
        self.assertEqual(result["canon-1"]["last_task"]["phase"], "reviewer")
        self.assertEqual(result["canon-1"]["last_by_phase"]["fixer"]["http_status"], 200)
        self.assertEqual(result["canon-1"]["last_by_phase"]["reviewer"]["http_status"], 401)
        self.assertEqual(result["xmp-2"]["last_task"]["http_status"], 500)

    def test_latest_phase_task_keeps_fixer_and_reviewer_results_separate(self):
        stat = {
            "last_by_phase": {
                "fixer": {"phase": "fixer", "epoch": 10, "http_status": 200},
                "reviewer": {"phase": "reviewer", "epoch": 11, "http_status": 500},
                "critique": {"phase": "critique", "epoch": 12, "http_status": 200},
            }
        }
        self.assertEqual(dashboard.latest_phase_task(stat, "fixer")["http_status"], 200)
        self.assertEqual(dashboard.latest_phase_task(stat, "reviewer", "critique")["phase"], "critique")

    def test_manifest_stats_reads_only_newly_appended_records(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.log"
            path.write_text("2026-08-04T12:00:00 phase=fixer worker=canon-1 provider=x OK\n")
            dashboard.manifest_stats(path)
            with path.open("a") as handle:
                handle.write("2026-08-04T12:01:00 phase=reviewer worker=canon-1 provider=x OK\n")
            result = dashboard.manifest_stats(path)
        self.assertEqual(result["canon-1"]["fixer_calls"], 1)
        self.assertEqual(result["canon-1"]["reviewer_calls"], 1)

    def test_process_argument_parsers_handle_live_worker_and_merger_commands(self):
        self.assertEqual(dashboard.worker_from_command("uv run scripts/model_fix_loop.py --only-format JPEG --worker-id canon-7"), "canon-7")
        self.assertEqual(dashboard.worker_from_command("scripts/model_fix_loop.py --format DNG"), "DNG")
        self.assertEqual(dashboard.squad_from_command("scripts/squad_merge_loop.py --squad sony-minolta --infinite"), "sony-minolta")
        self.assertTrue(dashboard.runs_script("python -u /fleet/scripts/parallel_model_fix_loop.py --infinite", "parallel_model_fix_loop.py"))
        self.assertFalse(dashboard.runs_script("rg 'overlord_sweep.py'", "overlord_sweep.py"))
        self.assertAlmostEqual(dashboard.cpu_seconds("0:01.25"), 1.25)
        self.assertAlmostEqual(dashboard.cpu_seconds("1-00:00:01"), 86401.0)

    def test_repo_from_script_command_uses_the_dispatcher_checkout(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "fleet-main"
            (root / "scripts").mkdir(parents=True)
            command = f"python -u {root}/scripts/parallel_model_fix_loop.py --infinite"
            self.assertEqual(
                dashboard.repo_from_script_command(command, "parallel_model_fix_loop.py"), root,
            )

    def test_parse_origin_main_divergence(self):
        self.assertEqual(dashboard.parse_origin_main_divergence("7\t2\n"), (7, 2))
        self.assertIsNone(dashboard.parse_origin_main_divergence("not a count"))

    def test_worker_runtime_state_only_marks_a_long_quiet_live_process_frozen(self):
        process = {"pid": 42, "elapsed_seconds": 1900, "cpu_percent": 0.0}
        state, reason = dashboard.worker_runtime_state(process, {"epoch": 100}, now=2000)
        self.assertEqual(state, "frozen")
        self.assertIn("no recorded task progress", reason)
        state, _ = dashboard.worker_runtime_state({"pid": 42, "elapsed_seconds": 60, "cpu_percent": 0.0}, {"epoch": 0}, now=2000)
        self.assertEqual(state, "running")

    def test_deepest_descendant_tracks_the_live_leaf_process(self):
        rows = {
            1: {"pid": 1, "ppid": 0, "cpu_percent": 0.0, "memory_bytes": 1, "command": "python3 worker.py"},
            2: {"pid": 2, "ppid": 1, "cpu_percent": 0.0, "memory_bytes": 1, "command": "bash build.sh"},
            3: {"pid": 3, "ppid": 2, "cpu_percent": 0.0, "memory_bytes": 1, "command": "python3 tool.py"},
            4: {"pid": 4, "ppid": 1, "cpu_percent": 0.0, "memory_bytes": 1, "command": "rustc crate.rs"},
            5: {"pid": 5, "ppid": 4, "cpu_percent": 5.0, "memory_bytes": 1, "command": "mold -o app"},
        }
        active = dashboard.deepest_descendant(rows, rows[1])
        self.assertEqual(active["pid"], 5)
        annotated = dashboard.child_process_usage(rows, rows[1])
        self.assertEqual(annotated["active_pid"], 5)
        self.assertEqual(annotated["active_command"], "mold -o app")
        worker = dashboard.item("worker:test", "worker", "test", annotated)
        reviewer = dashboard.item("reviewer:test", "reviewer", "test reviewer", annotated)
        merger = dashboard.item("merger:test", "merger", "test merger", annotated)
        self.assertEqual(worker["pid"], 5)
        self.assertEqual(reviewer["pid"], 5)
        self.assertEqual(merger["pid"], 1)

    def test_patch_stats_splits_git_applied_from_apply_failed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in ("t-canon-1-applied.diff", "t-canon-1-rejected.diff", "t-xmp-2-applied.diff"):
                (root / name).write_text("diff")
            result = dashboard.patch_stats(root, ["canon-1", "xmp-2"])
        self.assertEqual(result["canon-1"]["patches_found"], 2)
        self.assertEqual(result["canon-1"]["patches_applied"], 1)
        self.assertEqual(result["canon-1"]["patches_apply_failed"], 1)
        self.assertEqual(result["xmp-2"]["patches_found"], 1)
        self.assertEqual(result["xmp-2"]["patches_applied"], 1)
        self.assertEqual(result["xmp-2"]["patches_apply_failed"], 0)

    def test_active_tag_claims_exposes_a_worker_primary_tag_and_cluster(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "model-fix-tag-state.json"
            path.write_text(
                '{"PDF:XMP:DateTime":{"claimed_by":"pdf-1","claimed_at":10000},'
                '"PDF:XMP:CreateDate":{"claimed_by":"pdf-1","claimed_at":10000},'
                '"JPEG:EXIF:Old":{"claimed_by":"jpeg-1","claimed_at":1000}}'
            )
            result = dashboard.active_tag_claims(path, now=10001)
        self.assertEqual(result["pdf-1"]["tag"], "PDF:XMP:DateTime")
        self.assertEqual(result["pdf-1"]["tags"], ["PDF:XMP:DateTime", "PDF:XMP:CreateDate"])
        self.assertNotIn("jpeg-1", result)

    def test_publisher_events_are_unique_by_pr_number(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "fleet-up.log"
            path.write_text(
                "2026-08-03T12:00:00 [dispatcher] created PR #400\n"
                "2026-08-04T12:00:00 [dispatcher] merged PR #400\n"
                "2026-08-04T13:00:00 [dispatcher] opened PR #401\n"
            )
            with mock.patch.object(dashboard, "github_recent_prs", return_value=[]):
                result = dashboard.publisher_stats(path, Path(tmp))
        self.assertEqual(result["prs_made"], 2)
        self.assertEqual(result["last_pr"]["number"], "401")

    def test_publisher_stats_exposes_recent_github_pr_rows(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "fleet-up.log"
            path.write_text("2026-08-04T12:00:00 [dispatcher] created PR #400\n")
            recent = [{"number": "444", "name": "fix/pdf-dark", "title": "fix(PDF): preserve Dark metadata", "timestamp": "2026-08-04T13:00:00Z", "epoch": 1.0, "url": "https://github.com/swack-tools/oxidex/pull/444"}]
            with mock.patch.object(dashboard, "github_recent_prs", return_value=recent):
                result = dashboard.publisher_stats(path, Path(tmp))
        self.assertEqual(result["recent_prs"], recent)
        self.assertIn("GitHub", result["source"])

    def test_controls_reject_unknown_and_unrelated_processes(self):
        with self.assertRaises(ValueError):
            dashboard.safe_process({"components": []}, "missing")
        data = {"components": [{"id": "bad", "process": {"pid": 8, "command": "python unrelated.py"}}]}
        with self.assertRaises(ValueError):
            dashboard.safe_process(data, "bad")

    def test_queue_stats_keeps_only_latest_verdict_for_a_patch(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            (home / "logs").mkdir()
            (home / "logs" / "judgment-queue.jsonl").write_text(
                '{"patch_id":"a","verdict":"queued","ts_epoch":1,"format":"JPEG"}\n'
                '{"patch_id":"a","verdict":"promoted","ts_epoch":2,"format":"JPEG"}\n'
                '{"patch_id":"b","verdict":"queued","ts_epoch":3,"format":"PDF"}\n'
            )
            result = dashboard.queue_stats(home)
        self.assertEqual(result["judgment_depth"], 1)
        self.assertEqual(result["event_count"], 3)
        self.assertEqual(result["queued"][0]["patch_id"], "b")

    def test_flow_stats_reports_api_total_queue_and_role_drilldowns(self):
        components = [
            dashboard.item("dispatcher", "dispatcher", "Task dispatcher", None, metrics={"events": 8}),
            dashboard.item("worker:jpeg", "worker", "jpeg", None, metrics={"patches_found": 4}),
            dashboard.item("reviewer:jpeg", "reviewer", "jpeg reviewer", None, metrics={"patches_applied": 3}),
            dashboard.item("merger:media", "merger", "media merger", None),
        ]
        flow = dashboard.flow_stats(
            components,
            {"jpeg": {"fixer_calls": 7, "reviewer_calls": 2, "critique_calls": 1}},
            {"prs_made": 5, "last_pr": None, "source": "test"},
            {"judgment_depth": 2, "event_count": 9, "blocked_squads": [], "batch_commits": 3, "queued": []},
        )
        nodes = flow["nodes"]
        self.assertEqual(nodes["api"]["detail"]["total"], 10)
        self.assertEqual(nodes["dispatcher"]["headline"], "0/1 running")
        self.assertEqual(nodes["dispatcher"]["detail"]["events"], 8)
        self.assertEqual(nodes["dispatcher"]["members"][0]["label"], "Task dispatcher")
        self.assertEqual(nodes["workers"]["label"], "Fixer workers")
        self.assertEqual(nodes["workers"]["detail"]["patches_found"], 4)
        self.assertEqual(nodes["reviewers"]["detail"]["calls"], 3)
        self.assertEqual(nodes["reviewers"]["members"][0]["label"], "jpeg reviewer")
        self.assertEqual(nodes["queue"]["detail"]["event_count"], 9)
        self.assertEqual(nodes["main"]["detail"]["merged_prs"], 5)

    def test_waiting_publisher_is_reported_as_dispatcher_owned_not_idle(self):
        components = [dashboard.item("publisher", "publisher", "PR publisher", None, status="waiting")]
        flow = dashboard.flow_stats(
            components, {}, {"prs_made": 5, "last_pr": None, "source": "test"},
            {"judgment_depth": 0, "event_count": 0, "blocked_squads": [], "batch_commits": 0, "queued": []},
        )
        self.assertEqual(flow["nodes"]["publisher"]["detail"]["status"], "waiting")
        self.assertIn("waiting for dispatcher", flow["nodes"]["publisher"]["summary"])

    def test_page_renders_group_flow_with_component_drilldowns(self):
        page = dashboard.page("test-token", False).decode()
        self.assertIn("Each box is a component group with aggregate statistics", page)
        self.assertIn("pos = {supervisor:", page)
        self.assertIn("node.headline", page)
        self.assertIn("click to inspect", page)
        self.assertIn("flagged changes", page)

    def test_page_renders_card_grid_with_fleet_controls_and_api_feed(self):
        page = dashboard.page("test-token", True).decode()
        self.assertIn('"test-token"', page)
        self.assertIn("CONTROLS=true", page)
        self.assertIn("id=cards", page)
        self.assertIn("function createCard", page)
        self.assertIn("id=fleet-banner", page)
        self.assertIn("Fleet is stopped", page)
        self.assertIn("fleet-scale-form", page)
        self.assertIn("fleet-scale-workers", page)
        self.assertIn("component-detail-start", page)
        self.assertIn("mid-round-badge", page)
        self.assertIn("bootout + bootstrap", page)
        self.assertIn("Workers are mid-round. Apply anyway (loses in-flight work)?", page)
        self.assertIn("merger cap unsupported by deployed fleet_up.sh", page)
        self.assertIn("Takes effect on next start", page)
        self.assertIn("apiRequestTable", page)
        self.assertIn("Last 20 requests", page)
        self.assertIn("Provider / model", page)
        self.assertIn("How a fixer worker reaches origin/main", page)
        self.assertIn("Dispatcher-owned", page)
        self.assertIn("Waiting is healthy", page)
        self.assertIn("function specificGuide", page)
        self.assertIn("readableStatCards", page)
        read_only = dashboard.page("other-token", False).decode()
        self.assertIn("CONTROLS=false", read_only)


class ManifestFeedTests(unittest.TestCase):
    def setUp(self):
        dashboard._api_recent_requests.clear()

    def test_feed_records_carry_request_fields_and_await_tag_stamp(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.log"
            path.write_text(
                "2026-08-08T19:30:26 phase=fixer worker=canon-1 tier=T1 provider=p model=m-1 prompt_chars=1000 elapsed=27.8s reply_chars=42 OK\n"
                "2026-08-08T19:31:00 phase=reviewer worker=xmp-2 provider=p http_status=500 ERROR=HTTP\n"
            )
            dashboard.manifest_stats(path)
        records = list(dashboard._api_recent_requests)
        self.assertEqual(len(records), 2)
        first, second = records
        self.assertEqual(first["worker"], "canon-1")
        self.assertEqual(first["tier"], "T1")
        self.assertEqual(first["model"], "m-1")
        self.assertEqual(first["prompt_chars"], 1000)
        self.assertEqual(first["reply_chars"], 42)
        self.assertEqual(first["elapsed"], "27.8s")
        self.assertTrue(first["unstamped"])
        self.assertIsNone(first["tag"])
        # Old-format lines still ingest, with the newer fields explicitly absent.
        self.assertIsNone(second["tier"])
        self.assertIsNone(second["prompt_chars"])
        self.assertEqual(second["http_status"], 500)

    def test_feed_is_bounded_to_the_last_twenty_requests(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.log"
            path.write_text("".join(f"2026-08-08T19:30:{i % 60:02d} phase=fixer worker=w-{i} provider=p OK\n" for i in range(30)))
            dashboard.manifest_stats(path)
        records = list(dashboard._api_recent_requests)
        self.assertEqual(len(records), dashboard.API_RECENT_REQUESTS)
        self.assertEqual(records[-1]["worker"], "w-29")


class FleetControlValidationTests(unittest.TestCase):
    def test_valid_count_rejects_bools_floats_strings_and_out_of_range(self):
        self.assertTrue(dashboard._valid_count(3, 1, 64))
        self.assertTrue(dashboard._valid_count(1, 1, 64))
        self.assertTrue(dashboard._valid_count(64, 1, 64))
        self.assertFalse(dashboard._valid_count(True, 1, 64))
        self.assertFalse(dashboard._valid_count(False, 0, 64))
        self.assertFalse(dashboard._valid_count(3.0, 1, 64))
        self.assertFalse(dashboard._valid_count("3", 1, 64))
        self.assertFalse(dashboard._valid_count(0, 1, 64))
        self.assertFalse(dashboard._valid_count(65, 1, 64))
        self.assertFalse(dashboard._valid_count(None, 1, 64))

    def test_host_allowed_admits_only_loopback_hosts(self):
        for header in ("127.0.0.1", "127.0.0.1:8765", "localhost", "localhost:8765", "LOCALHOST:8765", "[::1]", "[::1]:8765"):
            self.assertTrue(dashboard.host_allowed(header), header)
        for header in (None, "", "evil.example", "evil.example:8765", "127.0.0.1.evil.example", "::1", "localhost.evil.example"):
            self.assertFalse(dashboard.host_allowed(header), repr(header))

    def test_fleet_parse_arguments_reads_workers_mergers_and_squad_mode(self):
        args = ["/bin/bash", "/deploy/scripts/fleet_up.sh", "--workers", "12", "--mergers", "4", "--squad-mode", "--config", "/deploy/config.toml"]
        parsed = dashboard.fleet_parse_arguments(args)
        self.assertEqual(parsed, {"configured_workers": 12, "configured_mergers": 4, "squad_mode": True})
        self.assertEqual(dashboard.fleet_parse_arguments(["--workers", "x"]), {"configured_workers": None, "configured_mergers": None, "squad_mode": False})

    def test_fleet_squad_total_prefers_the_config_flag_then_legacy_squads_toml(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / "config.toml"
            config.write_text("[fleet]\nworkers = 3\n[squads.canon]\nx = 1\n[squads.nikon]\nx = 1\n")
            args = ["/bin/bash", f"{tmp}/scripts/fleet_up.sh", "--config", str(config)]
            self.assertEqual(dashboard.fleet_squad_total(args), 2)
            legacy_dir = Path(tmp) / "scripts"
            legacy_dir.mkdir()
            (legacy_dir / "squads.toml").write_text("[squads.a]\n[squads.b]\n[squads.c]\n")
            self.assertEqual(dashboard.fleet_squad_total(["/bin/bash", f"{legacy_dir}/fleet_up.sh"]), 3)
            self.assertIsNone(dashboard.fleet_squad_total(["/bin/bash", "/nowhere/fleet_up.sh"]))

    def test_dispatcher_pgids_parses_list_and_dict_shapes(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            (home / "logs").mkdir()
            pgids = home / "logs" / "dispatcher-pgids.json"
            self.assertEqual(dashboard.dispatcher_pgids(home), [])
            self.assertFalse(dashboard.fleet_mid_round(home))
            pgids.write_text("[1234, 5678]")
            self.assertEqual(dashboard.dispatcher_pgids(home), [1234, 5678])
            self.assertTrue(dashboard.fleet_mid_round(home))
            pgids.write_text('{"pgids": [99]}')
            self.assertEqual(dashboard.dispatcher_pgids(home), [99])
            pgids.write_text("[]")
            self.assertFalse(dashboard.fleet_mid_round(home))


class FleetPlistWriteTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.plist = Path(self.tmp.name) / "com.oxidex.fleet.plist"
        make_plist(self.plist)
        patcher = mock.patch.object(dashboard, "FLEET_PLIST", self.plist)
        patcher.start()
        self.addCleanup(patcher.stop)
        dashboard._invalidate_fleet_config_cache()

    def read_args(self) -> list[str]:
        with self.plist.open("rb") as handle:
            return plistlib.load(handle)["ProgramArguments"]

    def test_scale_mutation_replaces_workers_and_inserts_mergers_pair(self):
        backup = dashboard.fleet_plist_write(dashboard._scale_mutation(12, 4))
        args = self.read_args()
        self.assertEqual(args[args.index("--workers") + 1], "12")
        mergers_index = args.index("--mergers")
        self.assertEqual(args[mergers_index + 1], "4")
        # Deterministic position: immediately after the --workers value.
        self.assertEqual(mergers_index, args.index("--workers") + 2)
        self.assertTrue((self.plist.parent / backup).exists(), "timestamped backup must exist")
        # A second write replaces the existing pair instead of inserting again.
        dashboard.fleet_plist_write(dashboard._scale_mutation(None, 7))
        args = self.read_args()
        self.assertEqual(args.count("--mergers"), 1)
        self.assertEqual(args[args.index("--mergers") + 1], "7")

    def test_write_refuses_unrecognized_plist_shapes(self):
        make_plist(self.plist, label="com.other.agent")
        with self.assertRaises(dashboard.FleetControlError) as ctx:
            dashboard.fleet_plist_write(dashboard._scale_mutation(5, None))
        self.assertEqual(ctx.exception.status, HTTPStatus.INTERNAL_SERVER_ERROR)
        self.assertIn("structure not recognized", ctx.exception.payload["error"])
        make_plist(self.plist, args=["/bin/bash", "/deploy/fleet_up.sh"])  # no --workers pair
        with self.assertRaises(dashboard.FleetControlError):
            dashboard.fleet_plist_write(dashboard._scale_mutation(5, None))

    def test_write_detects_a_concurrent_editor_and_makes_no_changes(self):
        original = self.plist.read_bytes()

        def racing_mutate(data):
            # A concurrent editor lands between the load and the re-stat.
            self.plist.touch()
            data["ProgramArguments"][3] = "99"

        with self.assertRaises(dashboard.FleetControlError) as ctx:
            dashboard.fleet_plist_write(racing_mutate)
        self.assertEqual(ctx.exception.status, HTTPStatus.CONFLICT)
        self.assertIn("changed on disk", ctx.exception.payload["error"])
        self.assertEqual(self.plist.read_bytes(), original)

    def test_missing_plist_is_refused_never_created(self):
        self.plist.unlink()
        with self.assertRaises(dashboard.FleetControlError) as ctx:
            dashboard.fleet_plist_write(dashboard._scale_mutation(5, None))
        self.assertEqual(ctx.exception.status, HTTPStatus.INTERNAL_SERVER_ERROR)
        self.assertFalse(self.plist.exists(), "the dashboard must never create a plist")


class FleetStartLadderTests(unittest.TestCase):
    def setUp(self):
        dashboard._invalidate_fleet_config_cache()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.plist = Path(self.tmp.name) / "com.oxidex.fleet.plist"
        make_plist(self.plist)
        patcher = mock.patch.object(dashboard, "FLEET_PLIST", self.plist)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_refuses_when_a_foreign_fleet_runs_outside_launchd(self):
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=False), \
             mock.patch.object(dashboard, "live_fleet_pids", return_value=[111, 222]):
            with self.assertRaises(dashboard.FleetControlError) as ctx:
                dashboard.fleet_start(Path("/repo"), Path("/home"))
        self.assertEqual(ctx.exception.status, HTTPStatus.CONFLICT)
        self.assertIn("outside launchd", ctx.exception.payload["error"])
        self.assertIn("111", ctx.exception.payload["error"])

    def test_loaded_and_running_is_an_idempotent_no_op(self):
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=True), \
             mock.patch.object(dashboard, "live_fleet_pids", return_value=[111]), \
             mock.patch.object(dashboard.subprocess, "run") as run:
            message = dashboard.fleet_start(Path("/repo"), Path("/home"))
        self.assertEqual(message, "fleet already running")
        run.assert_not_called()

    def test_loaded_but_stopped_uses_kickstart_without_dash_k(self):
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=True), \
             mock.patch.object(dashboard, "live_fleet_pids", return_value=[]), \
             mock.patch.object(dashboard.subprocess, "run", return_value=completed()) as run:
            message = dashboard.fleet_start(Path("/repo"), Path("/home"))
        self.assertEqual(message, "kickstarted")
        argv = run.call_args.args[0]
        self.assertEqual(argv, ["launchctl", "kickstart", f"gui/{dashboard.os.getuid()}/com.oxidex.fleet"])
        self.assertNotIn("-k", argv)

    def test_not_loaded_with_plist_bootstraps_and_reports_configured_workers(self):
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=False), \
             mock.patch.object(dashboard, "live_fleet_pids", return_value=[]), \
             mock.patch.object(dashboard, "fleet_bootstrap", return_value=completed()) as bootstrap, \
             mock.patch.object(dashboard, "fleet_config", return_value={"configured_workers": 3}):
            message = dashboard.fleet_start(Path("/repo"), Path("/home"))
        bootstrap.assert_called_once()
        self.assertIn("bootstrapped", message)
        self.assertIn("3 workers", message)

    def test_missing_plist_is_refused_never_invented(self):
        self.plist.unlink()
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=False), \
             mock.patch.object(dashboard, "live_fleet_pids", return_value=[]), \
             mock.patch.object(dashboard, "fleet_bootstrap") as bootstrap:
            with self.assertRaises(dashboard.FleetControlError) as ctx:
                dashboard.fleet_start(Path("/repo"), Path("/home"))
        bootstrap.assert_not_called()
        self.assertIn("not found", ctx.exception.payload["error"])
        self.assertIn("will not create one", ctx.exception.payload["error"])

    def test_disabled_service_is_surfaced_never_auto_enabled(self):
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=False), \
             mock.patch.object(dashboard, "live_fleet_pids", return_value=[]), \
             mock.patch.object(dashboard, "fleet_bootstrap", return_value=completed(returncode=1, stderr="Bootstrap failed: 125")), \
             mock.patch.object(dashboard, "fleet_service_disabled", return_value=True):
            with self.assertRaises(dashboard.FleetControlError) as ctx:
                dashboard.fleet_start(Path("/repo"), Path("/home"))
        self.assertIn("disabled", ctx.exception.payload["error"])
        self.assertIn("launchctl enable", ctx.exception.payload["error"])

    def test_fleet_down_boots_out_when_loaded_and_uses_down_flag_otherwise(self):
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=True), \
             mock.patch.object(dashboard, "fleet_bootout", return_value=completed()) as bootout:
            message = dashboard.fleet_down(Path("/repo"))
        bootout.assert_called_once()
        self.assertIn("bootout", message)
        with mock.patch.object(dashboard, "fleet_service_loaded", return_value=False), \
             mock.patch.object(dashboard.subprocess, "run", return_value=completed()) as run:
            message = dashboard.fleet_down(Path("/repo"))
        self.assertIn("--down", run.call_args.args[0])
        self.assertIn("fleet_up.sh", run.call_args.args[0][0])


class FleetScaleActionTests(unittest.TestCase):
    CONFIG = {
        "plist_present": True, "service_loaded": True, "service_disabled": False,
        "configured_workers": 3, "configured_mergers": None, "squad_mode": True,
        "capabilities": {"mergers_flag": True}, "plist_mtime": 1, "squad_total": 14,
    }

    def setUp(self):
        dashboard._invalidate_fleet_config_cache()
        self.home = Path(tempfile.mkdtemp())

    def scale(self, request, config=None, mid_round=False, bootout=None, bootstrap=None):
        with mock.patch.object(dashboard, "fleet_config", return_value=dict(config or self.CONFIG)), \
             mock.patch.object(dashboard, "fleet_mid_round", return_value=mid_round), \
             mock.patch.object(dashboard, "fleet_plist_write", return_value="plist.bak-120000") as self.write, \
             mock.patch.object(dashboard, "fleet_bootout", return_value=bootout or completed()) as self.bootout, \
             mock.patch.object(dashboard, "fleet_bootstrap", return_value=bootstrap or completed()) as self.bootstrap:
            return dashboard.fleet_scale_action(dict(request), self.home)

    def scale_error(self, request, **kwargs):
        with self.assertRaises(dashboard.FleetControlError) as ctx:
            self.scale(request, **kwargs)
        return ctx.exception

    def test_rejects_type_confused_and_out_of_range_counts(self):
        for bad_workers in (True, False, 3.0, "3", 0, 65, None):
            exc = self.scale_error({"id": "fleet", "action": "scale", "workers": bad_workers})
            self.assertEqual(exc.status, HTTPStatus.BAD_REQUEST, repr(bad_workers))
            self.assertIn("integer between 1 and 64", exc.payload["error"])
        exc = self.scale_error({"id": "fleet", "action": "scale", "mergers": 33})
        self.assertIn("integer between 0 and 32", exc.payload["error"])
        exc = self.scale_error({"id": "fleet", "action": "scale"})
        self.assertIn("at least one of workers or mergers", exc.payload["error"])
        exc = self.scale_error({"id": "fleet", "action": "scale", "workers": 5, "force": "yes"})
        self.assertIn("force must be a boolean", exc.payload["error"])
        exc = self.scale_error({"id": "fleet", "action": "scale", "workers": 5, "shell": "rm -rf"})
        self.assertIn("unexpected fields", exc.payload["error"])

    def test_mergers_are_capability_gated_on_the_deployed_script(self):
        config = dict(self.CONFIG, capabilities={"mergers_flag": False})
        exc = self.scale_error({"id": "fleet", "action": "scale", "mergers": 4}, config=config)
        self.assertEqual(exc.status, HTTPStatus.CONFLICT)
        self.assertIn("does not support --mergers", exc.payload["error"])

    def test_scale_to_current_values_is_a_no_op_without_a_write(self):
        status, payload = self.scale({"id": "fleet", "action": "scale", "workers": 3})
        self.assertEqual(status, HTTPStatus.OK)
        self.assertIn("already configured", payload["message"])
        self.assertFalse(payload["applied"])
        self.write.assert_not_called()
        self.bootout.assert_not_called()

    def test_scale_on_a_stopped_fleet_saves_without_starting_it(self):
        config = dict(self.CONFIG, service_loaded=False)
        status, payload = self.scale({"id": "fleet", "action": "scale", "workers": 12}, config=config)
        self.assertEqual(status, HTTPStatus.OK)
        self.assertIn("takes effect on next start", payload["message"])
        self.assertFalse(payload["applied"])
        self.write.assert_called_once()
        self.bootout.assert_not_called()
        self.bootstrap.assert_not_called()

    def test_mid_round_scale_persists_config_but_requires_force_to_apply(self):
        exc = self.scale_error({"id": "fleet", "action": "scale", "workers": 12}, mid_round=True)
        self.assertEqual(exc.status, HTTPStatus.CONFLICT)
        self.assertIn("mid-round", exc.payload["error"])
        self.assertIn("config saved but not applied", exc.payload["error"])
        self.assertFalse(exc.payload["applied"])
        self.assertEqual(exc.payload["backup"], "plist.bak-120000")
        status, payload = self.scale({"id": "fleet", "action": "scale", "workers": 12, "force": True}, mid_round=True)
        self.assertEqual(status, HTTPStatus.OK)
        self.assertTrue(payload["applied"])
        self.bootout.assert_called_once()
        self.bootstrap.assert_called_once()

    def test_applied_scale_restarts_via_bootout_then_bootstrap(self):
        status, payload = self.scale({"id": "fleet", "action": "scale", "workers": 12})
        self.assertEqual(status, HTTPStatus.OK)
        self.assertIn("workers 3 -> 12", payload["message"])
        self.assertTrue(payload["applied"])
        self.bootout.assert_called_once()
        self.bootstrap.assert_called_once()

    def test_bootout_failure_reports_step_and_running_old_config(self):
        exc = self.scale_error({"id": "fleet", "action": "scale", "workers": 12}, bootout=completed(returncode=1, stderr="boom"))
        self.assertEqual(exc.payload["step"], "bootout")
        self.assertEqual(exc.payload["fleet_state"], "running-old-config")
        self.assertIn("backup", exc.payload)

    def test_bootstrap_failure_reports_fleet_down_with_config_staged(self):
        exc = self.scale_error({"id": "fleet", "action": "scale", "workers": 12}, bootstrap=completed(returncode=1, stderr="boom"))
        self.assertEqual(exc.payload["step"], "bootstrap")
        self.assertEqual(exc.payload["fleet_state"], "down-new-config-staged")
        self.assertIn("'start' retries the bootstrap", exc.payload["error"])


class HandlerRoutingTests(unittest.TestCase):
    """HTTP-level checks with every fleet mutation mocked out."""

    @classmethod
    def setUpClass(cls):
        cls.server = dashboard.Server(("127.0.0.1", 0), Path(tempfile.mkdtemp()), True)
        cls.thread = threading.Thread(target=cls.server.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True)
        cls.thread.start()
        cls.port = cls.server.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def post(self, body, token=None, host=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        headers = {"content-type": "application/json", "x-control-token": token if token is not None else self.server.token}
        if host is not None:
            headers["Host"] = host
        connection.request("POST", "/api/control", json.dumps(body), headers)
        response = connection.getresponse()
        payload = json.loads(response.read() or b"{}")
        connection.close()
        return response.status, payload

    def test_non_loopback_host_header_is_rejected_before_anything_else(self):
        status, payload = self.post({"id": "fleet", "action": "start"}, host="dashboard.evil.example")
        self.assertEqual(status, HTTPStatus.FORBIDDEN)
        self.assertIn("Host header", payload["error"])

    def test_bad_token_is_rejected(self):
        status, payload = self.post({"id": "fleet", "action": "start"}, token="wrong")
        self.assertEqual(status, HTTPStatus.FORBIDDEN)
        self.assertIn("token", payload["error"])

    def test_start_and_scale_are_fleet_only(self):
        status, payload = self.post({"id": "worker:canon-1", "action": "start"})
        self.assertEqual(status, HTTPStatus.BAD_REQUEST)
        self.assertIn("applies only to the fleet component", payload["error"])
        status, payload = self.post({"id": "dispatcher", "action": "scale", "workers": 5})
        self.assertEqual(status, HTTPStatus.BAD_REQUEST)

    def test_unknown_action_is_rejected(self):
        status, payload = self.post({"id": "fleet", "action": "enable"})
        self.assertEqual(status, HTTPStatus.BAD_REQUEST)

    def test_start_rejects_extra_fields(self):
        status, payload = self.post({"id": "fleet", "action": "start", "workers": 5})
        self.assertEqual(status, HTTPStatus.BAD_REQUEST)
        self.assertIn("no fields beyond", payload["error"])

    def test_start_routes_to_fleet_start_and_returns_its_message(self):
        with mock.patch.object(dashboard, "fleet_start", return_value="bootstrapped; fleet starting with 3 workers") as start:
            status, payload = self.post({"id": "fleet", "action": "start"})
        self.assertEqual(status, HTTPStatus.OK)
        self.assertIn("bootstrapped", payload["message"])
        start.assert_called_once()

    def test_start_surfaces_fleet_control_errors_as_structured_json(self):
        error = dashboard.FleetControlError(HTTPStatus.CONFLICT, {"error": "a fleet is already running outside launchd (pids 42)"})
        with mock.patch.object(dashboard, "fleet_start", side_effect=error):
            status, payload = self.post({"id": "fleet", "action": "start"})
        self.assertEqual(status, HTTPStatus.CONFLICT)
        self.assertIn("outside launchd", payload["error"])

    def test_scale_routes_through_fleet_scale_action(self):
        with mock.patch.object(dashboard, "fleet_scale_action", return_value=(HTTPStatus.OK, {"message": "scaled workers 3 -> 12 (persisted; fleet restarted)", "applied": True})) as scale:
            status, payload = self.post({"id": "fleet", "action": "scale", "workers": 12})
        self.assertEqual(status, HTTPStatus.OK)
        self.assertTrue(payload["applied"])
        self.assertEqual(scale.call_args.args[0]["workers"], 12)

    def test_control_operations_are_single_flight(self):
        self.assertTrue(self.server.control_lock.acquire(blocking=False))
        try:
            with mock.patch.object(dashboard, "fleet_start") as start:
                status, payload = self.post({"id": "fleet", "action": "start"})
            start.assert_not_called()
            self.assertEqual(status, HTTPStatus.CONFLICT)
            self.assertIn("in flight", payload["error"])
        finally:
            self.server.control_lock.release()
        # The lock releases after each action: a follow-up request succeeds.
        with mock.patch.object(dashboard, "fleet_start", return_value="fleet already running"):
            status, _ = self.post({"id": "fleet", "action": "start"})
        self.assertEqual(status, HTTPStatus.OK)


if __name__ == "__main__":
    unittest.main()
