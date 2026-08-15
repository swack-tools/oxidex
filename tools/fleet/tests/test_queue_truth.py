#!/usr/bin/env python3
"""Tests for ARCH-FIX R4 ("the queue consults what is known"):

  * workqueue.py's claim-exclusion key-match fix -- a live fleetd claim
    (work_key like "staging/foo") must exclude its branch from a queue
    computed by a SECOND, independent Hub object (a different local
    workdir, standing in for a second host), in both directions, and must
    keep excluding it for as long as the claim's renewer keeps it alive
    past what its original ttl alone would have covered (T1's renewer is
    what makes that continued exclusion true, not just the ttl at claim
    time).
  * fleetd.py's verdict-aware gate-candidate selection -- a branch with a
    memoized PASS/FAIL for the current tip is parked as
    awaiting_train/needs_author instead of being offered as gate work
    again; ABORT is not memoized and the branch is offered as usual.
  * cli.py's `fleet status` WORK column (from live claims' work_keys) and
    its AWAITING/NEEDS_AUTH/KILLED columns (from heartbeats).

Everything here runs against throwaway `git init --bare` fixture repos
(via test_queue.QueueTestCase / test_fleetd.make_fixture_hub) -- never the
production hub. Plain unittest, standard library only.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import contextlib
import io
import json
import re
import subprocess
import sys
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import cli  # noqa: E402
import fleetd  # noqa: E402
import verdict  # noqa: E402
from claim import Claim  # noqa: E402
from fleetlib import Hub  # noqa: E402
from test_fleetd import FleetdBase, HUB_TIP_REF  # noqa: E402
from test_queue import QueueTestCase  # noqa: E402
from workqueue import Queue  # noqa: E402


def _kill_worker_process(worker) -> None:
    """Best-effort teardown for a subprocess-backed Worker started
    directly via fleetd.start_gate/start_agent in a test (i.e. not through
    reconcile_once's stub-gate + stop-file protocol)."""
    if worker is None or worker.popen is None:
        return
    worker.popen.terminate()
    try:
        worker.popen.wait(timeout=10)
    except subprocess.TimeoutExpired:
        worker.popen.kill()
        worker.popen.wait(timeout=10)


# ----------------------------------------------------------------------- #
# R4 item 1 -- key-match round trip
# ----------------------------------------------------------------------- #


class TestClaimExclusionRoundTrip(QueueTestCase):
    """A claim created via fleetd's REAL start_gate (work_key = the full
    branch name, e.g. "staging/mike" -- exactly what start_gate sets)
    excludes that branch from workqueue.Queue.compute() run against a
    SECOND Hub instance with its own, independent local workdir."""

    def setUp(self):
        super().setUp()
        self.other_workdir = str(Path(self._tmp_root) / "other-host-cache")
        self.log_dir = Path(self._tmp_root) / "gatelogs"

    def _other_hub(self) -> Hub:
        return Hub(url=self.hub_path, workdir=self.other_workdir)

    def test_claim_appears_branch_excluded_claim_released_branch_returns(self):
        self.commit_on("staging/mike", "mike.txt", "mike")

        gate_command = [sys.executable, "-c", "import time; time.sleep(60)"]
        worker = fleetd.start_gate(
            self.hub, "staging/mike", "roundtrip-tag", gate_command, "host-a", self.log_dir,
        )
        self.assertIsNotNone(worker, "fleetd.start_gate must have won the claim")
        self.assertEqual(worker.claim.work_key, "staging/mike")
        try:
            during = Queue(self._other_hub(), tip_ref=self.TIP_REF).compute()
            self.assertNotIn(
                "mike", during,
                "a live fleetd claim (work_key='staging/mike') must exclude the "
                "branch from a queue computed by a SECOND, independent host -- "
                "this is the exact key format start_gate writes, not the bare "
                "slug or the full ref workqueue used to compare against",
            )
        finally:
            worker.claim.release()
            _kill_worker_process(worker)

        after = Queue(self._other_hub(), tip_ref=self.TIP_REF).compute()
        self.assertIn(
            "mike", after,
            "once the claim is released, the branch must be queueable again on "
            "the same second host (proves the earlier exclusion tracked the "
            "claim's liveness, not some caching artifact)",
        )

    def test_agent_claim_on_a_promoted_branch_also_excludes_it(self):
        """start_agent's work_key is the same full-branch-name convention
        as start_gate's -- the fix is shared by construction (both funnel
        through Queue._live_claim_work_keys), but this pins the agent path
        explicitly since it is a distinct call site in fleetd.py."""
        self.commit_on("staging/oscar", "oscar.txt", "oscar")
        claim = Claim(
            self.hub, kind="agent", key="oscar-key",
            work_kind="agent", work_key="staging/oscar", ttl=600,
        )
        claim.acquire()
        try:
            result = Queue(self._other_hub(), tip_ref=self.TIP_REF).compute()
            self.assertNotIn("oscar", result)
        finally:
            claim.release()
        after = Queue(self._other_hub(), tip_ref=self.TIP_REF).compute()
        self.assertIn("oscar", after)


class TestRenewedClaimStaysExcluded(QueueTestCase):
    """R2 (T1) made renewal automatic inside acquire(); this pins the
    composition with R4's fix -- a claim that is CONTINUOUSLY renewed
    stays excluded from a second host's queue past what its ORIGINAL ttl
    alone would have covered. Before T1's fix, nothing renewed a claim
    fleetd held via acquire()/acquire_or_reap(), so _live_claim_work_keys'
    is_expired filter would have silently stopped excluding this branch
    once the original ttl passed -- a second, independent path to the
    same double-gate outcome the claim exists to prevent."""

    def test_renewed_claim_survives_past_its_original_ttl(self):
        self.commit_on("staging/november", "november.txt", "november")
        other = Hub(url=self.hub_path, workdir=str(Path(self._tmp_root) / "other-host-cache"))

        claim = Claim(
            self.hub, kind="gate", key="november-key",
            work_kind="gate", work_key="staging/november",
            ttl=3, renew_interval=1,
        )
        claim.acquire()
        try:
            self.assertTrue(claim.renewer_running(), "acquire() must start the renewer (R2)")
            deadline = time.time() + 6.5  # > 2x the original 3s ttl
            saw_a_check_past_original_ttl = False
            checkpoint_after_ttl = time.time() + 3.5
            while time.time() < deadline:
                result = Queue(other, tip_ref=self.TIP_REF).compute()
                self.assertNotIn(
                    "november", result,
                    "a continuously-renewed claim must stay excluded past its "
                    "original ttl -- only a lapsed renewer should ever let this "
                    "branch reappear in another host's queue",
                )
                if time.time() >= checkpoint_after_ttl:
                    saw_a_check_past_original_ttl = True
                time.sleep(0.5)
            self.assertTrue(
                saw_a_check_past_original_ttl,
                "test did not actually run long enough to prove anything past the original ttl",
            )
            self.assertFalse(claim.lost, "a healthy, continuously-renewed claim must never be lost")
        finally:
            claim.release()

        after = Queue(other, tip_ref=self.TIP_REF).compute()
        self.assertIn("november", after, "once released, the branch must be queueable again")


# ----------------------------------------------------------------------- #
# R4 item 2 -- verdict-aware selection
# ----------------------------------------------------------------------- #


def _write_gatelog(
    log_dir: Path,
    tag: str,
    *,
    branch: str,
    base_tip: str,
    result: str,
    gate_version: str,
    tree_sha: str = "d" * 40,
    platform_id: str = "plat-x",
) -> None:
    log_dir.mkdir(parents=True, exist_ok=True)
    payload = {
        "tree_sha": tree_sha,
        "base_tip": base_tip,
        "branch": branch,
        "result": result,
        "stage": "test",
        "gate_version": gate_version,
        "rustc_id": "r",
        "platform_id": platform_id,
        "host": "some-host",
        "duration_s": 1,
        "write_set": [],
    }
    (log_dir / f"gate-{tag}.json").write_text(json.dumps(payload))


class TestVerdictAwareSelection(FleetdBase):
    def setUp(self):
        super().setUp()
        self.repo_root = Path(__file__).resolve().parents[3]
        self.gate_version = fleetd._gate_version(self.repo_root)
        self.assertIsNotNone(
            self.gate_version, "fixture host must be able to read gate_version.txt"
        )
        self.tip_sha = self.hub.sha(HUB_TIP_REF)
        self.log_dir = self.tmp / "logs"

    def test_pass_memo_parks_branch_as_awaiting_train_not_offered(self):
        _write_gatelog(
            self.log_dir, "prior-pass", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_PASS, gate_version=self.gate_version,
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(
            res.started, [], f"a PASS-memoized branch must not be dispatched: {res.started}"
        )
        self.assertIn("staging/one", res.awaiting_train)
        self.assertEqual(res.needs_author, [])

    def test_fail_memo_parks_branch_as_needs_author_not_offered(self):
        _write_gatelog(
            self.log_dir, "prior-fail", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_FAIL, gate_version=self.gate_version,
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(
            res.started, [], f"a FAIL-memoized branch must not be dispatched: {res.started}"
        )
        self.assertIn("staging/one", res.needs_author)
        self.assertEqual(res.awaiting_train, [])

    def test_abort_memo_still_offers_the_branch(self):
        _write_gatelog(
            self.log_dir, "prior-abort", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_ABORT, gate_version=self.gate_version,
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(
            len(res.started), 1, f"an ABORT verdict must not block dispatch: refused={res.refused}"
        )
        self.assertEqual(res.awaiting_train, [])
        self.assertEqual(res.needs_author, [])

    def test_memo_from_a_different_gate_version_is_ignored(self):
        """A verdict recorded under a stale GATE_VERSION says nothing
        about what the CURRENT gate would decide -- must not block."""
        _write_gatelog(
            self.log_dir, "prior-pass-old-version", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_PASS, gate_version=f"{self.gate_version}-stale",
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"stale-gate_version memo must not block: {res.refused}")
        self.assertEqual(res.awaiting_train, [])

    def test_shared_cache_confirms_and_overrides_a_stale_local_pass(self):
        """A local gatelogs JSON says PASS for this exact tree, but the
        SHARED verdict cache holds a FAIL for that identical
        (tree_sha, gate_version, platform_id) key -- e.g. another host's
        more complete run. classify_branch must prefer the hub's answer
        over the stale local recall."""
        tree_sha = "a" * 40
        platform_id = "plat-shared"
        _write_gatelog(
            self.log_dir, "prior-pass-stale", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_PASS, gate_version=self.gate_version,
            tree_sha=tree_sha, platform_id=platform_id,
        )
        stored = verdict.store(self.hub, {
            "tree_sha": tree_sha, "base_tip": self.tip_sha, "branch": "staging/one",
            "result": verdict.RESULT_FAIL, "stage": "test", "gate_version": self.gate_version,
            "rustc_id": "r", "platform_id": platform_id, "host": "other-host",
            "duration_s": 1, "write_set": [],
        })
        self.assertEqual(stored, "created")

        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(
            res.started, [], f"the shared hub FAIL must win over the stale local PASS: {res.started}"
        )
        self.assertIn("staging/one", res.needs_author)
        self.assertEqual(res.awaiting_train, [])

    def test_no_gatelogs_at_all_is_a_no_op(self):
        """Backward compatibility: a host with no ~/gatelogs history yet
        (fresh install, or a stub gate that never writes JSON, as every
        other test_fleetd.py test uses) must see zero change in
        candidate-selection behaviour."""
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1)
        self.assertEqual(res.awaiting_train, [])
        self.assertEqual(res.needs_author, [])


class TestScanGatelogsMemoUnit(unittest.TestCase):
    """Narrow unit coverage on _scan_gatelogs_memo/classify_branch as pure
    functions, independent of the full reconcile_once/fixture-hub path
    exercised above."""

    def setUp(self):
        import tempfile
        self._tmp = tempfile.TemporaryDirectory()
        self.log_dir = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_most_recent_file_wins_per_key(self):
        _write_gatelog(self.log_dir, "older", branch="staging/x", base_tip="tip1",
                        result=verdict.RESULT_FAIL, gate_version="4")
        # ensure a distinct, later mtime
        time.sleep(0.05)
        _write_gatelog(self.log_dir, "newer", branch="staging/x", base_tip="tip1",
                        result=verdict.RESULT_PASS, gate_version="4")
        memo = fleetd._scan_gatelogs_memo(self.log_dir)
        self.assertEqual(memo[("staging/x", "tip1")]["result"], verdict.RESULT_PASS)

    def test_malformed_json_is_skipped_not_raised(self):
        self.log_dir.mkdir(parents=True, exist_ok=True)
        (self.log_dir / "gate-bad.json").write_text("{not json")
        memo = fleetd._scan_gatelogs_memo(self.log_dir)
        self.assertEqual(memo, {})

    def test_missing_log_dir_is_a_no_op(self):
        memo = fleetd._scan_gatelogs_memo(self.log_dir / "does-not-exist")
        self.assertEqual(memo, {})

    def test_abort_result_leaves_no_memo_entry(self):
        _write_gatelog(self.log_dir, "aborted", branch="staging/y", base_tip="tip1",
                        result=verdict.RESULT_ABORT, gate_version="4")
        memo = fleetd._scan_gatelogs_memo(self.log_dir)
        self.assertNotIn(("staging/y", "tip1"), memo)


# ----------------------------------------------------------------------- #
# cli.py -- WORK column + awaiting/needs_author/killed counts
# ----------------------------------------------------------------------- #


def _parse_table(text: str):
    """Best-effort parse of cmd_status's two-space-joined, left-justified
    table back into (header, rows) so assertions can address a specific
    column by name rather than grepping for a bare substring."""
    lines = [ln for ln in text.splitlines() if ln.strip()]
    body = []
    header = None
    for ln in lines:
        if ln.startswith("QUEUE"):
            break
        cells = re.split(r"\s{2,}", ln.strip())
        if header is None:
            header = cells
        else:
            body.append(cells)
    return header or [], body


class TestCliStatusRendering(FleetdBase):
    def test_status_work_column_shows_the_live_claims_work_key(self):
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        header, rows = _parse_table(buf.getvalue())
        self.assertIn("WORK", header, f"header was: {header}")
        work_idx = header.index("WORK")
        host_idx = header.index("HOST")
        matching = [r for r in rows if len(r) > host_idx and r[host_idx] == self.host]
        self.assertEqual(len(matching), 1, f"rows: {rows}")
        self.assertEqual(matching[0][work_idx], "staging/one")

    def test_status_awaiting_needs_author_killed_columns_from_heartbeat(self):
        hb = {
            "gates_running": 0, "agents_running": 0, "free_gb": 10.0, "free_mem_gb": 10.0,
            "rustc_id": "r", "platform_id": "p", "owning_user": "t", "oracle_ok": None,
            "gate_version": "4", "tip_generation_seen": None,
            "awaiting_train": ["staging/a", "staging/b"],
            "needs_author": ["staging/c"],
            "killed_this_loop": 2,
            "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        self.assertTrue(self.hub.create(fleetd.HOSTS_PREFIX + self.host, hb))

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        header, rows = _parse_table(buf.getvalue())
        for col in ("AWAITING", "NEEDS_AUTH", "KILLED"):
            self.assertIn(col, header, f"header was: {header}")
        awaiting_idx = header.index("AWAITING")
        needs_idx = header.index("NEEDS_AUTH")
        killed_idx = header.index("KILLED")
        host_idx = header.index("HOST")
        matching = [r for r in rows if len(r) > host_idx and r[host_idx] == self.host]
        self.assertEqual(len(matching), 1, f"rows: {rows}")
        row = matching[0]
        self.assertEqual(row[awaiting_idx], "2")
        self.assertEqual(row[needs_idx], "1")
        self.assertEqual(row[killed_idx], "2")

    def test_status_work_column_is_dash_with_no_live_claims(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        out = buf.getvalue()
        self.assertIn("(no host heartbeats yet", out)


if __name__ == "__main__":
    unittest.main()
