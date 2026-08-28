#!/usr/bin/env python3
"""Tests for tools/fleet/claim.py.

Everything here runs against a throwaway `git init --bare` repo created in
`setUp` -- never the production hub (work2.oxidex.net). Same fixture
discipline as `test_fleetlib.py`: `setUp` asserts the bare repo's path
lives under the system temp directory before any test body runs.

Two tests use real concurrency (a `ProcessPoolExecutor`, not mocks) because
the property under test -- "two simulated hosts cannot hold the same claim
key" and "a second reaper with a stale expect_sha fails cleanly" -- is
exactly the kind of thing a mock would define into passing rather than
prove.

Plain `unittest`, standard library only (no pytest in this environment).

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from concurrent.futures import ProcessPoolExecutor, as_completed
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from claim import (  # noqa: E402
    Claim,
    ClaimHeldError,
    claim_ref,
    compute_platform_id,
    compute_rustc_id,
    is_claim_live,
    is_expired,
    is_workdir_claimed,
    parse_claim_ref,
    reap_expired,
)
from fleetlib import Hub  # noqa: E402
from _env import HermeticCase  # noqa: E402
from _fixtures import make_hub  # noqa: E402
from _mp import pool_context  # noqa: E402

# Explicit, per-call-site start method; nothing here touches the
# process-global default. See `tests/_mp.py`.
_MP_CONTEXT = pool_context()


def _run_git(args, cwd=None, input_bytes=None):
    return subprocess.run(args, cwd=cwd, input=input_bytes, capture_output=True)


class ClaimTestCase(HermeticCase):
    """Base fixture: a throwaway bare repo standing in for the hub."""

    def setUp(self):
        super().setUp()
        self._tmp_root = tempfile.mkdtemp(prefix="claim-test-")
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        self.workdir = str(Path(self._tmp_root) / "cache")

        init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        # The single most important assertion in this file: never let the
        # fixture -- or by extension any test -- point at anything but a
        # temp path. This is the guard against ever contacting the
        # production hub (work2.oxidex.net) from a test run.
        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

        self.hub = make_hub(self, self.hub_path, workdir=self.workdir)

    def tearDown(self):
        shutil.rmtree(self._tmp_root, ignore_errors=True)

    def fresh_hub(self) -> Hub:
        """A second Hub instance with its own local cache, same remote --
        simulates a second host talking to the same hub.
        """
        other_workdir = tempfile.mkdtemp(prefix="claim-test-cache2-")
        self.addCleanup(shutil.rmtree, other_workdir, ignore_errors=True)
        return Hub(url=self.hub_path, workdir=other_workdir)


# --------------------------------------------------------------------- #
# Fixture guard
# --------------------------------------------------------------------- #


class TestFixtureGuard(ClaimTestCase):
    def test_hub_is_a_temp_path_never_the_production_hub(self):
        resolved = str(Path(self.hub.url).resolve())
        self.assertTrue(resolved.startswith(str(Path(tempfile.gettempdir()).resolve())))
        self.assertNotIn("work2.oxidex.net", resolved)


# --------------------------------------------------------------------- #
# Ref naming
# --------------------------------------------------------------------- #


class TestRefNaming(HermeticCase):
    def test_claim_ref_shape(self):
        self.assertEqual(claim_ref("gate", "abc123"), "refs/fleet/claims/gate/abc123")

    def test_claim_ref_rejects_slash_in_kind(self):
        with self.assertRaises(ValueError):
            claim_ref("gate/sub", "abc123")

    def test_parse_claim_ref_round_trips(self):
        ref = claim_ref("agent", "route-legacy-formats")
        self.assertEqual(parse_claim_ref(ref), ("agent", "route-legacy-formats"))

    def test_parse_claim_ref_rejects_unrelated_ref(self):
        self.assertIsNone(parse_claim_ref("refs/heads/staging/foo"))
        self.assertIsNone(parse_claim_ref("refs/fleet/claims/onlykind"))


# --------------------------------------------------------------------- #
# Acquire / renew / release lifecycle
# --------------------------------------------------------------------- #


class TestClaimLifecycle(ClaimTestCase):
    def test_acquire_then_second_acquire_raises(self):
        c1 = Claim(self.hub, "gate", "treeA", work_key="branch-a")
        c1.acquire()
        c2 = Claim(self.fresh_hub(), "gate", "treeA", work_key="branch-a")
        with self.assertRaises(ClaimHeldError):
            c2.acquire()
        c1.release()

    def test_release_then_acquire_succeeds(self):
        c1 = Claim(self.hub, "gate", "treeB", work_key="branch-b")
        c1.acquire()
        self.assertTrue(c1.release())

        c2 = Claim(self.fresh_hub(), "gate", "treeB", work_key="branch-b")
        c2.acquire()  # must not raise -- the ref is gone
        c2.release()

    def test_context_manager_releases_on_normal_exit(self):
        with Claim(self.hub, "gate", "treeC", work_key="branch-c", renew_interval=3600):
            self.assertIsNotNone(self.hub.sha(claim_ref("gate", "treeC")))
        self.assertIsNone(self.hub.sha(claim_ref("gate", "treeC")))

    def test_context_manager_releases_on_exception(self):
        with self.assertRaises(RuntimeError):
            with Claim(self.hub, "gate", "treeD", work_key="branch-d", renew_interval=3600):
                raise RuntimeError("simulated failure inside the claimed work")
        self.assertIsNone(self.hub.sha(claim_ref("gate", "treeD")))

    def test_renew_extends_expiry_and_keeps_ref_alive(self):
        claim = Claim(self.hub, "gate", "treeE", work_key="branch-e", ttl=600)
        claim.acquire()
        first_payload = self.hub.read(claim.ref)
        time.sleep(0.05)  # guarantee a strictly later "now" for the renewal
        self.assertTrue(claim.renew())
        second_payload = self.hub.read(claim.ref)
        self.assertGreater(second_payload["expires_at"], first_payload["expires_at"])
        claim.release()

    def test_release_is_idempotent(self):
        claim = Claim(self.hub, "gate", "treeF", work_key="branch-f")
        claim.acquire()
        self.assertTrue(claim.release())
        self.assertTrue(claim.release())  # second call: no-op, not an error

    def test_payload_carries_required_fields(self):
        claim = Claim(
            self.hub,
            "gate",
            "treeG",
            work_kind="gate",
            work_key="branch-g",
            gate_version="v7",
            rustc_id="deadbeef",
            platform_id="cafef00d",
            workdir="nc-3",
        )
        claim.acquire()
        payload = self.hub.read(claim.ref)
        for field in (
            "holder_host",
            "pid",
            "pgid",
            "work_kind",
            "work_key",
            "started_at",
            "expires_at",
            "gate_version",
            "rustc_id",
            "platform_id",
        ):
            self.assertIn(field, payload, msg=f"missing {field!r} in claim payload")
        self.assertEqual(payload["work_kind"], "gate")
        self.assertEqual(payload["work_key"], "branch-g")
        self.assertEqual(payload["gate_version"], "v7")
        self.assertEqual(payload["rustc_id"], "deadbeef")
        self.assertEqual(payload["platform_id"], "cafef00d")
        self.assertEqual(payload["workdir"], "nc-3")
        claim.release()


# --------------------------------------------------------------------- #
# Real concurrency: two simulated hosts, one key
# --------------------------------------------------------------------- #


def _try_acquire_in_subprocess(hub_path, kind, key):
    """Runs in a child process (its own Hub, its own cache dir) -- stands
    in for a second host racing to claim the same key.
    """
    workdir = tempfile.mkdtemp(prefix="claim-test-race-")
    try:
        hub = Hub(url=hub_path, workdir=workdir)
        claim = Claim(hub, kind, key, work_key=key)
        try:
            claim.acquire()
            return True
        except ClaimHeldError:
            return False
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


class TestConcurrentClaims(ClaimTestCase):
    def test_two_simulated_hosts_cannot_hold_the_same_key(self):
        n_workers = 6
        with ProcessPoolExecutor(max_workers=n_workers, mp_context=_MP_CONTEXT) as pool:
            futures = [
                pool.submit(_try_acquire_in_subprocess, self.hub_path, "gate", "contested-tree")
                for _ in range(n_workers)
            ]
            results = [f.result() for f in as_completed(futures)]

        self.assertEqual(
            sum(1 for r in results if r is True),
            1,
            msg=f"expected exactly one winner, got {results}",
        )
        self.assertEqual(sum(1 for r in results if r is False), n_workers - 1)

        # And the payload on the hub reflects a single winner's claim.
        self.assertIsNotNone(self.hub.sha(claim_ref("gate", "contested-tree")))


# --------------------------------------------------------------------- #
# Reaping expired claims
# --------------------------------------------------------------------- #


class TestReaping(ClaimTestCase):
    def _create_expired_claim(self, kind="gate", key="stale-tree", **extra):
        past = datetime.now(timezone.utc) - timedelta(seconds=10)
        payload = {
            "holder_host": "dead-host",
            "pid": 12345,
            "pgid": 12345,
            "work_kind": kind,
            "work_key": key,
            "started_at": (past - timedelta(seconds=600)).isoformat(),
            "expires_at": past.isoformat(),
            "gate_version": "v1",
            "rustc_id": "abc",
            "platform_id": "abc:host",
        }
        payload.update(extra)
        ok = self.hub.create(claim_ref(kind, key), payload)
        self.assertTrue(ok)
        return claim_ref(kind, key)

    def test_expired_claim_is_reaped(self):
        ref = self._create_expired_claim()
        reaped = reap_expired(self.hub)
        self.assertIn(ref, reaped)
        self.assertIsNone(self.hub.sha(ref))

    def test_live_claim_is_not_reaped(self):
        claim = Claim(self.hub, "gate", "fresh-tree", work_key="fresh-tree", ttl=600)
        claim.acquire()
        reaped = reap_expired(self.hub)
        self.assertNotIn(claim.ref, reaped)
        self.assertIsNotNone(self.hub.sha(claim.ref))
        claim.release()

    def test_expired_claim_is_reapable_exactly_once(self):
        ref = self._create_expired_claim(key="reap-once")
        first = reap_expired(self.hub)
        self.assertIn(ref, first)

        # Nothing left to reap: the ref is gone, so a second pass over the
        # same (now-empty) listing finds nothing.
        second = reap_expired(self.hub)
        self.assertNotIn(ref, second)
        self.assertEqual(second, [])

    def test_second_reaper_with_stale_expect_sha_fails_cleanly(self):
        """Simulates two reapers that both observed the claim before either
        deleted it: both hold the same `expect_sha`, but only the first
        `--force-with-lease` delete can win.
        """
        ref = self._create_expired_claim(key="race-reap")
        observed_sha = self.hub.sha(ref)
        self.assertIsNotNone(observed_sha)

        first_delete = self.hub.delete(ref, expect_sha=observed_sha)
        self.assertTrue(first_delete)

        # Second reaper, holding the SAME stale expect_sha (it observed the
        # claim before the first reaper deleted it) -- must fail cleanly
        # (return False), never raise.
        second_delete = self.hub.delete(ref, expect_sha=observed_sha)
        self.assertFalse(second_delete)

    def test_reap_scoped_by_kind_ignores_other_kinds(self):
        gate_ref = self._create_expired_claim(kind="gate", key="scoped-a")
        agent_ref = self._create_expired_claim(kind="agent", key="scoped-b")

        reaped = reap_expired(self.hub, kind="gate")
        self.assertIn(gate_ref, reaped)
        self.assertNotIn(agent_ref, reaped)
        self.assertIsNotNone(self.hub.sha(agent_ref))  # untouched

        # Clean up the remaining one.
        reap_expired(self.hub, kind="agent")


def _try_reap_in_subprocess(hub_path, kind):
    workdir = tempfile.mkdtemp(prefix="claim-test-reap-race-")
    try:
        hub = Hub(url=hub_path, workdir=workdir)
        return reap_expired(hub, kind=kind)
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


class TestConcurrentReap(ClaimTestCase):
    def test_two_simulated_reapers_only_one_wins_the_delete(self):
        past = datetime.now(timezone.utc) - timedelta(seconds=10)
        payload = {
            "holder_host": "dead-host",
            "pid": 1,
            "pgid": 1,
            "work_kind": "gate",
            "work_key": "double-reap-tree",
            "started_at": (past - timedelta(seconds=600)).isoformat(),
            "expires_at": past.isoformat(),
            "gate_version": "v1",
            "rustc_id": "abc",
            "platform_id": "abc:host",
        }
        ref = claim_ref("gate", "double-reap-tree")
        self.assertTrue(self.hub.create(ref, payload))

        n_workers = 5
        with ProcessPoolExecutor(max_workers=n_workers, mp_context=_MP_CONTEXT) as pool:
            futures = [
                pool.submit(_try_reap_in_subprocess, self.hub_path, "gate")
                for _ in range(n_workers)
            ]
            results = [f.result() for f in as_completed(futures)]

        reaped_counts = [1 if ref in r else 0 for r in results]
        self.assertEqual(sum(reaped_counts), 1, msg=f"expected exactly one reaper to win, got {results}")
        self.assertIsNone(self.hub.sha(ref))


# --------------------------------------------------------------------- #
# Crashed holder
# --------------------------------------------------------------------- #


class TestCrashedHolder(ClaimTestCase):
    def test_crashed_holder_leaves_a_claim_that_expires_and_is_reapable(self):
        # Simulate a holder that crashed before it ever reached release():
        # acquire directly (bypassing the context manager and its renewer
        # thread), and simply never call release() or __exit__. A very
        # short ttl stands in for "time passed" without a real sleep.
        claim = Claim(self.hub, "gate", "crash-tree", work_key="crash-branch", ttl=0)
        claim.acquire()
        self.assertIsNotNone(self.hub.sha(claim.ref))

        # ttl=0 means expires_at == started_at; by the time we check, it's
        # already at-or-past expiry.
        payload = self.hub.read(claim.ref)
        self.assertTrue(is_expired(payload))

        self.assertFalse(is_claim_live(self.hub, "gate", "crash-tree"))

        reaped = reap_expired(self.hub)
        self.assertIn(claim.ref, reaped)
        self.assertIsNone(self.hub.sha(claim.ref))

    def test_acquire_or_reap_recovers_a_crashed_holders_slot(self):
        crashed = Claim(self.hub, "gate", "crash-tree-2", work_key="crash-branch-2", ttl=0)
        crashed.acquire()  # "crashes" here -- never releases

        successor = Claim(self.fresh_hub(), "gate", "crash-tree-2", work_key="crash-branch-2", ttl=600)
        successor.acquire_or_reap()  # must reap the expired claim and win
        self.assertIsNotNone(self.hub.sha(successor.ref))
        successor.release()


# --------------------------------------------------------------------- #
# Workdir liveness interlock (incident 12: rm -rf ~/tgt/* killed 7 gates)
# --------------------------------------------------------------------- #


class TestWorkdirLiveness(ClaimTestCase):
    def test_workdir_named_by_a_live_claim_is_claimed(self):
        claim = Claim(self.hub, "gate", "wd-tree", work_key="wd-branch", workdir="nc-3", ttl=600)
        claim.acquire()
        self.assertTrue(is_workdir_claimed(self.hub, "nc-3"))
        self.assertFalse(is_workdir_claimed(self.hub, "nc-4"))
        claim.release()
        self.assertFalse(is_workdir_claimed(self.hub, "nc-3"))

    def test_workdir_named_only_by_an_expired_claim_is_not_claimed(self):
        claim = Claim(self.hub, "gate", "wd-tree-2", work_key="wd-branch-2", workdir="nc-7", ttl=0)
        claim.acquire()
        # Expired immediately (ttl=0) -- the directory must NOT read as
        # claimed, or a real reaper would never be able to clean it up.
        self.assertFalse(is_workdir_claimed(self.hub, "nc-7"))


# --------------------------------------------------------------------- #
# rustc_id vs platform_id
# --------------------------------------------------------------------- #


class TestToolchainIdentity(HermeticCase):
    MAC_VV = (
        "rustc 1.97.1 (abc123 2026-01-01)\n"
        "binary: rustc\n"
        "commit-hash: abc123\n"
        "commit-date: 2026-01-01\n"
        "host: aarch64-apple-darwin\n"
        "release: 1.97.1\n"
    )
    LINUX_VV = (
        "rustc 1.97.1 (abc123 2026-01-01)\n"
        "binary: rustc\n"
        "commit-hash: abc123\n"
        "commit-date: 2026-01-01\n"
        "host: x86_64-unknown-linux-gnu\n"
        "release: 1.97.1\n"
    )

    def test_rustc_id_same_across_hosts_on_identical_release(self):
        self.assertEqual(compute_rustc_id(self.MAC_VV), compute_rustc_id(self.LINUX_VV))

    def test_platform_id_differs_across_hosts_on_identical_release(self):
        self.assertNotEqual(compute_platform_id(self.MAC_VV), compute_platform_id(self.LINUX_VV))

    def test_rustc_id_and_platform_id_are_not_the_same_value(self):
        self.assertNotEqual(compute_rustc_id(self.MAC_VV), compute_platform_id(self.MAC_VV))


if __name__ == "__main__":
    unittest.main()
