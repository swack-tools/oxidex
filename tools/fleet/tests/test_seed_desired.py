#!/usr/bin/env python3
"""Unit tests for tools/fleet/rollout/seed_desired.py.

`seed_desired.py` had no dedicated test module before PLAN Stage 1 task 6
(`server_candidates` + `train_platforms`, SPEC §3.1). Everything here runs
against a throwaway `git init --bare` repo under the system temp dir,
never a real hub -- same non-negotiable guard as `tests/test_fleetlib.py`
and `tests/test_drift_hook.py`.

Run with:
    ( cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_seed_desired )
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # tools/fleet

from rollout import seed_desired  # noqa: E402
from _env import HermeticCase  # noqa: E402
from _fixtures import make_hub, within_sweep  # noqa: E402

DESIRED_REF = seed_desired.DESIRED_REF


def _run_git(args, cwd=None):
    return subprocess.run(["git"] + args, cwd=cwd, capture_output=True)


class SeedDesiredTestCase(HermeticCase):
    """Base fixture: a throwaway bare repo standing in for the state hub."""

    def setUp(self):
        super().setUp()
        self._tmp_root = tempfile.mkdtemp(prefix="seed-desired-test-")
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.hub_path = str(Path(self._tmp_root) / "hub.git")

        init = _run_git(["init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        # Same non-negotiable guard as test_fleetlib.py/test_drift_hook.py:
        # never let this fixture resolve to anything but a temp path.
        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

        self.hub = make_hub(self, self.hub_path, workdir=Path(self._tmp_root) / "cache")

        # A real FLEET_HUB_URL in the ambient shell must never leak into a
        # test that is exercising the "no --hub given" code path.
        self._hub_url_env = os.environ.get("FLEET_HUB_URL")
        os.environ.pop("FLEET_HUB_URL", None)

    def tearDown(self):
        if self._hub_url_env is None:
            os.environ.pop("FLEET_HUB_URL", None)
        else:
            os.environ["FLEET_HUB_URL"] = self._hub_url_env

    def run_main(self, argv):
        """`seed_desired.main(argv)`, with stdout/stderr captured so a test
        can assert on them without polluting the suite's own output."""
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = seed_desired.main(argv)
        return rc, out.getvalue(), err.getvalue()


# ---------------------------------------------------------------------- #
# SPEC §3.1 additions: server_candidates / train_platforms shape, no hub
# involved -- these are properties of the SEED constant itself.
# ---------------------------------------------------------------------- #


class TestSeedPayloadShape(SeedDesiredTestCase):
    def test_server_candidates_excludes_macos_and_laptop_hosts(self):
        hosts = {c["host"] for c in seed_desired.SEED["server_candidates"]}
        # docs/FLEET.md L120: launchd (macOS) hosts are `oldair` and `m5`;
        # `m5` is additionally the maintainer's laptop -- SPEC §3.4's
        # separate, independent "laptops are never eligible" exclusion.
        self.assertNotIn("oldair", hosts)
        self.assertNotIn("m5", hosts)
        self.assertTrue(
            hosts.issubset(set(seed_desired.SEED["hosts"])),
            "every server candidate must be one of the seeded hosts",
        )
        self.assertTrue(hosts, "at least one Linux, non-laptop candidate must be seeded")

    def test_server_candidates_are_ranked_1_through_n_with_no_gaps(self):
        ranks = sorted(c["rank"] for c in seed_desired.SEED["server_candidates"])
        self.assertEqual(ranks, list(range(1, len(ranks) + 1)))

    def test_server_candidates_carry_no_invented_advertise_urls(self):
        # A tailnet/LAN IP is a runtime fact the elected server measures
        # for itself at election time (SPEC §3.4 step 1); this script must
        # never fabricate one -- see the module docstring's "never
        # approximate" note.
        for c in seed_desired.SEED["server_candidates"]:
            self.assertEqual(c["advertise_urls"], [])

    def test_train_platforms_seeded_empty_not_guessed(self):
        # A platform_id is sha256(rustc -vV) measured ON a specific host
        # (claim.compute_platform_id); nothing reachable from this script
        # can compute one truthfully, so it must not be invented.
        self.assertEqual(seed_desired.SEED["train_platforms"], [])

    def test_seed_is_json_serializable(self):
        json.dumps(seed_desired.SEED)  # must not raise

    def test_seed_still_carries_the_original_all_zero_host_counts(self):
        # The Stage 1 additions must not have disturbed the pre-existing
        # "fleetd's first reconcile is a no-op" guarantee.
        for host, entry in seed_desired.SEED["hosts"].items():
            self.assertEqual(entry["gates"], 0, host)
            self.assertEqual(entry["agents"], 0, host)


# ---------------------------------------------------------------------- #
# Dry-run (the default): prints, never writes.
# ---------------------------------------------------------------------- #


class TestDryRun(SeedDesiredTestCase):
    def test_dry_run_prints_the_seed_and_writes_nothing(self):
        rc, out, err = self.run_main(["--hub", self.hub_path])
        self.assertEqual(rc, 0)
        self.assertIn("server_candidates", out)
        self.assertIn("train_platforms", out)
        self.assertIn("DRY-RUN", err)
        self.assertIsNone(self.hub.sha(DESIRED_REF))

    def test_dry_run_printed_json_round_trips_to_the_seed(self):
        _rc, out, _err = self.run_main(["--hub", self.hub_path])
        printed = json.loads(out)
        self.assertEqual(printed, seed_desired.SEED)


# ---------------------------------------------------------------------- #
# --execute: the write path, and PLAN Stage 1 task 6's named property --
# seeding is idempotent, so a second run can never clobber a live desired
# state an operator has since edited with `fleet up/down`.
# ---------------------------------------------------------------------- #


class TestExecuteIdempotency(SeedDesiredTestCase):
    def test_execute_without_a_hub_fails_closed(self):
        rc, _out, err = self.run_main(["--execute"])
        self.assertEqual(rc, 2)
        self.assertIn("no hub URL", err)
        self.assertIsNone(self.hub.sha(DESIRED_REF))

    def test_first_execute_creates_the_ref_with_the_full_seed(self):
        rc, out, _err = self.run_main(["--hub", self.hub_path, "--execute"])
        self.assertEqual(rc, 0)
        self.assertIn("created", out)
        # `seed_desired.main` writes through its own plain Hub -- out of
        # band to the fixture server under FLEET_TEST_HUB=server -- so the
        # fixture hub's view converges at the next sweep; poll for it.
        stored = within_sweep(lambda: self.hub.read(DESIRED_REF), lambda v: v is not None)
        self.assertIsNotNone(stored)
        for key, value in seed_desired.SEED.items():
            self.assertEqual(stored[key], value)

    def test_second_execute_refuses_and_leaves_a_live_desired_state_untouched(self):
        first_rc, _out, _err = self.run_main(["--hub", self.hub_path, "--execute"])
        self.assertEqual(first_rc, 0)
        # The seed write is out-of-band (see above): poll the sha into
        # view before using it as a CAS witness -- handing a None witness
        # to `update` is a caller bug, not a race.
        sha_after_first = within_sweep(lambda: self.hub.sha(DESIRED_REF), lambda v: v is not None)
        self.assertIsNotNone(sha_after_first)

        # An operator raises a real host's targets in between -- exactly
        # the live state a careless re-seed must never step on.
        edited = dict(seed_desired.SEED)
        edited["generation"] = 2
        edited["hosts"] = {
            **edited["hosts"],
            "server": {"gates": 3, "agents": 1, "enabled": True},
        }
        self.assertTrue(self.hub.update(DESIRED_REF, edited, expect_sha=sha_after_first))
        sha_after_edit = self.hub.sha(DESIRED_REF)
        self.assertNotEqual(sha_after_edit, sha_after_first)

        second_rc, _out, err = self.run_main(["--hub", self.hub_path, "--execute"])
        self.assertEqual(second_rc, 3)
        self.assertIn("already exists", err)
        self.assertEqual(
            self.hub.sha(DESIRED_REF), sha_after_edit,
            "a second seed run must never overwrite a live desired state",
        )
        stored = self.hub.read(DESIRED_REF)
        self.assertEqual(stored["generation"], 2, "the operator's edit must survive re-seeding")

    def test_execute_is_idempotent_across_repeated_calls(self):
        rc1, _out, _err = self.run_main(["--hub", self.hub_path, "--execute"])
        self.assertEqual(rc1, 0)
        sha1 = self.hub.sha(DESIRED_REF)
        payload1 = self.hub.read(DESIRED_REF)

        for _ in range(3):
            rc_n, _out, err_n = self.run_main(["--hub", self.hub_path, "--execute"])
            self.assertEqual(rc_n, 3, "every re-run after the first must refuse, never overwrite")
            self.assertIn("already exists", err_n)

        self.assertEqual(self.hub.sha(DESIRED_REF), sha1)
        self.assertEqual(self.hub.read(DESIRED_REF), payload1)


if __name__ == "__main__":
    unittest.main()
