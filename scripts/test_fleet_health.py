#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Hermetic tests for fleet_health.py -- no live processes are probed
(kill_fn/argv_fn are always injected), no real ~/.oxidex is read, and the
squads.toml under test is a throwaway tempdir file.

The invariants that matter most here are the negative ones, because this
tool's whole reason to exist is that every earlier liveness check lied:

  * `pgrep -f` self-matched the asking shell, so a dead merger tier looked
    healthy (2026-07-26) -- hence pid_alive/pid_is_merger_for look up ONE
    known pid and are structurally incapable of matching the asker;
  * the fleet-up state file kept saying `running` for an hour after the
    2026-07-30 ENOSPC mass death, because its maintainer died with the
    mergers -- hence it supplies only a candidate PID whose life and exact
    argv must be validated;
  * run_locked removes the merger's OWN lock during every normal between-poll
    sleep -- hence a missing lock falls back to that validated candidate
    rather than becoming a false outage;
  * a threshold-only heartbeat check called two provably-alive mergers
    dead during a long cargo build -- hence THE PID DECIDES, and a stale
    heartbeat on a live pid is `stalled` (advisory), never an outage.
"""
import json
import os
import tempfile
import unittest
from pathlib import Path

import fleet_health as fh
import squad_merge_loop as sml

NOW = 1_000_000.0

#: Two squads, disjoint ownership: canon owns cr2 by module claim, xmp owns
#: xmp/svg. No format is contested, so the owner map is trivially stable.
SQUADS_TOML = """
[squads.canon]
modules = ["Canon", "CanonRaw"]
formats = ["CR2"]

[squads.xmp]
modules = ["XMP", "SVG"]
formats = ["XMP", "SVG", "CR2"]
"""


def alive_kill(pid, sig):
    return None


def dead_kill(pid, sig):
    raise ProcessLookupError(pid)


class FleetHealthTestCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.home = Path(self._tmp.name)
        self.squads_toml = self.home / "squads.toml"
        self.squads_toml.write_text(SQUADS_TOML)

    def write_lock(self, squad, pid=4242, heartbeat_age=10.0, raw=None):
        path = sml.merger_lock_path(self.home, squad)
        path.parent.mkdir(parents=True, exist_ok=True)
        if raw is None:
            info = {"pid": pid, "script_git_sha": "test-sha"}
            if heartbeat_age is not None:
                info["heartbeat_ts"] = NOW - heartbeat_age
            raw = json.dumps(info)
        path.write_text(raw)
        return path

    def write_batch_state(self, squad, blocked, age):
        path = sml.batch_state_path(self.home, squad)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(
            {"blocked": blocked, "last_batch_ts": NOW - age, "commits_since": 0}
        ))

    def write_fleet_state(self, squad, pid=4242, status="running"):
        path = self.home / "logs" / "fleet-up.state"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            f"supervisor\t111\trunning\tfleet_up.sh\n"
            f"merger:{squad}\t{pid}\t{status}\tsquad_merge_loop.py --squad {squad} \n"
        )
        return path


class PidAliveTests(unittest.TestCase):
    def test_own_pid_is_alive(self):
        self.assertTrue(fh.pid_alive(os.getpid()))

    def test_gone_pid_is_dead(self):
        self.assertFalse(fh.pid_alive(4242, kill_fn=dead_kill))

    def test_permission_error_means_alive_but_not_ours(self):
        def perm(pid, sig):
            raise PermissionError(pid)
        self.assertTrue(fh.pid_alive(4242, kill_fn=perm))

    def test_garbage_pids_are_dead_not_a_crash(self):
        for pid in (None, 0, -1, "4242"):
            self.assertFalse(fh.pid_alive(pid, kill_fn=alive_kill))


class PidIsMergerForTests(unittest.TestCase):
    def test_matching_merger_argv_matches(self):
        argv = "python3 scripts/squad_merge_loop.py --squad canon --infinite"
        self.assertTrue(fh.pid_is_merger_for(4242, "canon", argv_fn=lambda p: argv))

    def test_recycled_pid_running_something_else_does_not_match(self):
        self.assertFalse(fh.pid_is_merger_for(4242, "canon", argv_fn=lambda p: "sshd: allen"))

    def test_wrong_squads_merger_does_not_match(self):
        argv = "python3 scripts/squad_merge_loop.py --squad xmp --infinite"
        self.assertFalse(fh.pid_is_merger_for(4242, "canon", argv_fn=lambda p: argv))

    def test_unknown_argv_gets_the_benefit_of_the_doubt(self):
        # A ps failure under load, or a merger owned by another uid, must not
        # be reported as an outage on that evidence alone.
        self.assertTrue(fh.pid_is_merger_for(4242, "canon", argv_fn=lambda p: None))


class MergerStateTests(FleetHealthTestCase):
    def state(self, squad="canon", **kw):
        kw.setdefault("now", NOW)
        kw.setdefault("kill_fn", alive_kill)
        kw.setdefault("argv_fn", lambda p: None)
        return fh.merger_state(self.home, squad, **kw)

    def test_no_lock_file_is_down(self):
        s = self.state()
        self.assertFalse(s["alive"])
        self.assertIn("no lock file", s["reason"])

    def test_idle_between_polls_uses_validated_supervisor_pid(self):
        self.write_fleet_state("canon", pid=4242)
        argv = "python3 scripts/squad_merge_loop.py --squad canon --infinite"
        s = self.state(argv_fn=lambda p: argv)
        self.assertTrue(s["alive"])
        self.assertFalse(s["stalled"])
        self.assertEqual(s["pid"], 4242)
        self.assertIn("between polls", s["reason"])

    def test_stale_supervisor_state_with_dead_pid_is_down(self):
        self.write_fleet_state("canon", pid=4242)
        s = self.state(kill_fn=dead_kill)
        self.assertFalse(s["alive"])
        self.assertIn("gone", s["reason"])

    def test_stale_supervisor_state_with_recycled_pid_is_down(self):
        self.write_fleet_state("canon", pid=4242)
        s = self.state(argv_fn=lambda p: "some-unrelated-daemon")
        self.assertFalse(s["alive"])
        self.assertIn("recycled", s["reason"])

    def test_non_running_supervisor_entry_does_not_count(self):
        self.write_fleet_state("canon", pid=4242, status="giving_up")
        s = self.state()
        self.assertFalse(s["alive"])

    def test_corrupt_lock_is_down(self):
        self.write_lock("canon", raw="not json{")
        self.assertFalse(self.state()["alive"])

    def test_lock_with_dead_pid_is_down(self):
        # The SIGKILL corpse: the file outlived its writer.
        self.write_lock("canon", pid=4242)
        s = self.state(kill_fn=dead_kill)
        self.assertFalse(s["alive"])
        self.assertIn("gone", s["reason"])

    def test_lock_with_recycled_pid_is_down(self):
        self.write_lock("canon", pid=4242)
        s = self.state(argv_fn=lambda p: "some-unrelated-daemon")
        self.assertFalse(s["alive"])
        self.assertIn("recycled", s["reason"])

    def test_live_pid_with_fresh_heartbeat_is_alive(self):
        self.write_lock("canon", heartbeat_age=10.0)
        s = self.state()
        self.assertTrue(s["alive"])
        self.assertFalse(s["stalled"])

    def test_live_pid_with_stale_heartbeat_is_alive_but_stalled(self):
        # THE PID DECIDES: a long cargo build under load starves the
        # heartbeat while the merger is provably alive. Advisory, not outage.
        self.write_lock("canon", heartbeat_age=2400.0)
        s = self.state(stale_seconds=1800.0)
        self.assertTrue(s["alive"])
        self.assertTrue(s["stalled"])

    def test_live_pid_with_no_heartbeat_field_is_alive_not_stalled(self):
        self.write_lock("canon", heartbeat_age=None)
        s = self.state()
        self.assertTrue(s["alive"])
        self.assertFalse(s["stalled"])
        self.assertIsNone(s["age"])


class BlockedStateTests(FleetHealthTestCase):
    def test_no_batch_state_is_not_blocked(self):
        self.assertFalse(fh.blocked_state(self.home, "canon", now=NOW)["blocked"])

    def test_recently_blocked_is_a_hiccup_not_an_outage(self):
        # A failing batch check retries on its own cadence; a couple of
        # cycles is normal operation.
        self.write_batch_state("canon", blocked=True, age=600.0)
        self.assertFalse(
            fh.blocked_state(self.home, "canon", now=NOW, blocked_seconds=3600.0)["blocked"])

    def test_chronically_blocked_is_an_outage(self):
        # panasonic-leica sat blocked 4h+ on a duplicate-symbol error with
        # zero escalation -- a stall that looks alive.
        self.write_batch_state("canon", blocked=True, age=5 * 3600.0)
        self.assertTrue(
            fh.blocked_state(self.home, "canon", now=NOW, blocked_seconds=3600.0)["blocked"])

    def test_unblocked_batch_state_is_not_blocked(self):
        self.write_batch_state("canon", blocked=False, age=5 * 3600.0)
        self.assertFalse(fh.blocked_state(self.home, "canon", now=NOW)["blocked"])


class AssessTests(FleetHealthTestCase):
    def assess(self, **kw):
        kw.setdefault("now", NOW)
        kw.setdefault("kill_fn", alive_kill)
        kw.setdefault("argv_fn", lambda p: None)
        return fh.assess(self.home, self.squads_toml, **kw)

    def test_all_mergers_up_is_healthy(self):
        self.write_lock("canon")
        self.write_lock("xmp")
        report = self.assess()
        self.assertTrue(report["healthy"])
        self.assertEqual(report["unowned"], [])

    def test_dead_merger_strands_exactly_its_exclusive_formats(self):
        self.write_lock("xmp")  # canon has no lock -> down
        report = self.assess()
        self.assertFalse(report["healthy"])
        self.assertEqual(
            [(u["format"], u["cause"]) for u in report["unowned"]],
            [("CR2", "merger-down")],
        )

    def test_chronically_blocked_squad_is_the_same_alarm_as_a_death(self):
        self.write_lock("canon")
        self.write_lock("xmp")
        self.write_batch_state("xmp", blocked=True, age=5 * 3600.0)
        report = self.assess()
        self.assertFalse(report["healthy"])
        causes = {u["format"]: u["cause"] for u in report["unowned"]}
        self.assertEqual(causes.get("XMP"), "publication-blocked")
        self.assertEqual(causes.get("SVG"), "publication-blocked")
        self.assertNotIn("CR2", causes)

    def test_stalled_is_advisory_not_an_outage(self):
        self.write_lock("canon", heartbeat_age=2400.0)
        self.write_lock("xmp")
        report = self.assess(stale_seconds=1800.0)
        self.assertTrue(report["healthy"])
        self.assertEqual(report["stalled"], ["canon"])


class FormatsOwnedByTests(FleetHealthTestCase):
    def test_exclusive_ownership_follows_the_owner_map(self):
        self.assertEqual(fh.formats_owned_by("canon", self.squads_toml), ["CR2"])
        self.assertEqual(fh.formats_owned_by("xmp", self.squads_toml), ["SVG", "XMP"])

    def test_unknown_squad_owns_nothing(self):
        self.assertEqual(fh.formats_owned_by("nope", self.squads_toml), [])


class RenderAndMainTests(FleetHealthTestCase):
    def test_render_leads_with_the_alarm(self):
        self.write_lock("xmp")
        report = fh.assess(self.home, self.squads_toml, now=NOW,
                           kill_fn=alive_kill, argv_fn=lambda p: None)
        text = fh.render(report)
        self.assertIn("ALARM", text.splitlines()[0])
        self.assertIn("CR2", text)

    def test_render_healthy_says_ok(self):
        self.write_lock("canon")
        self.write_lock("xmp")
        report = fh.assess(self.home, self.squads_toml, now=NOW,
                           kill_fn=alive_kill, argv_fn=lambda p: None)
        self.assertTrue(fh.render(report).startswith("OK"))

    def test_main_formats_for_prints_and_exits_zero(self):
        import contextlib
        import io
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = fh.main(["--formats-for", "canon",
                          "--squads-toml", str(self.squads_toml),
                          "--home", str(self.home)])
        self.assertEqual(rc, 0)
        self.assertEqual(out.getvalue().split(), ["CR2"])

    def test_main_exit_status_is_the_health_verdict(self):
        # Everything down (no locks written): exit 1, and --json stays
        # machine-readable.
        import contextlib
        import io
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = fh.main(["--json", "--squads-toml", str(self.squads_toml),
                          "--home", str(self.home)])
        self.assertEqual(rc, 1)
        report = json.loads(out.getvalue())
        self.assertFalse(report["healthy"])


if __name__ == "__main__":
    unittest.main()
