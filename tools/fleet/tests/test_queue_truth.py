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
import os
import re
import subprocess
import sys
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import cli  # noqa: E402
import dispatch  # noqa: E402
import fleetd  # noqa: E402
import verdict  # noqa: E402
from claim import Claim, compute_platform_id  # noqa: E402
from fleetlib import Hub, HubUnreachableError  # noqa: E402
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


class VerdictSelectionBase(FleetdBase):
    """Fixture plumbing shared by every verdict-aware-selection case: the
    real merge tree `gate.sh` would key a verdict on, a force-push, and a
    peer's verdict in the shared cache."""

    def setUp(self):
        super().setUp()
        self.repo_root = Path(__file__).resolve().parents[3]
        self.gate_version = fleetd._gate_version(self.repo_root)
        self.assertIsNotNone(
            self.gate_version, "fixture host must be able to read gate_version.txt"
        )
        self.tip_sha = self.hub.sha(HUB_TIP_REF)
        self.log_dir = self.tmp / "logs"
        self.platform_id = compute_platform_id()
        # Process-lifetime memo keyed on content-addressed shas: correct in
        # production, but two fixtures built in the same second can mint the
        # same commit sha, so clear it between tests rather than let one
        # fixture's answer be served to another's.
        fleetd._MERGE_TREE_MEMO.clear()

    # ---- fixture helpers ------------------------------------------- #

    def branch_sha(self, branch="staging/one"):
        return self.hub.sha(f"refs/heads/{branch}")

    def merge_tree(self, branch="staging/one"):
        """The tree `gate.sh` would build and key its verdict on for this
        branch onto the current tip. Tests that assert a verdict is or is
        not honoured must use the REAL tree: a placeholder tree_sha is
        exactly the stale-verdict shape the sha-keying rejects, so a test
        written with one is testing the rejection, not the honouring."""
        ref = f"refs/heads/{branch}"
        self.assertTrue(dispatch._have_objects(self.hub, [HUB_TIP_REF, ref]))
        tree = dispatch.merge_tree_sha(self.hub, self.tip_sha, self.branch_sha(branch))
        self.assertIsNotNone(tree, "fixture must produce a clean merge tree")
        return tree

    def force_push(self, content, branch="staging/one"):
        """Rewrite `branch` to a brand-new commit on the same tip -- what an
        author does in response to NEEDS_AUTHOR. Returns the new sha."""
        work = self.tmp / f"fp-{content}"
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        subprocess.run(["git", "init", "-q", str(work)], check=True)
        subprocess.run(["git", "-C", str(work), "fetch", "-q", str(self.bare), HUB_TIP_REF],
                       check=True, env=env)
        subprocess.run(["git", "-C", str(work), "checkout", "-q", "-B", "fp", "FETCH_HEAD"],
                       check=True, env=env)
        (work / "g.txt").write_text(f"branch, {content}\n")
        subprocess.run(["git", "-C", str(work), "add", "."], check=True, env=env)
        subprocess.run(["git", "-C", str(work), "commit", "-qm", content], check=True, env=env)
        subprocess.run(["git", "-C", str(work), "push", "-q", "--force", str(self.bare),
                        f"HEAD:refs/heads/{branch}"], check=True, env=env)
        return self.branch_sha(branch)

    def store_verdict(self, result, tree_sha, *, host="other-host",
                      gate_version=None, platform_id=None):
        return verdict.store(self.hub, {
            "tree_sha": tree_sha, "base_tip": self.tip_sha, "branch": "staging/one",
            "result": result, "stage": "test",
            "gate_version": gate_version or self.gate_version,
            "rustc_id": "r", "platform_id": platform_id or self.platform_id,
            "host": host, "duration_s": 1, "write_set": [],
        })


class TestVerdictAwareSelection(VerdictSelectionBase):
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
        self.assertIn("staging/one", fleetd.branch_names(res.awaiting_train))
        self.assertEqual(res.needs_author, [])

    def test_fail_memo_parks_branch_as_needs_author_not_offered(self):
        """The local recall's FAIL is honoured -- but only against the tree
        it actually gated, which for an unchanged branch is the tree the
        current sha still merges to."""
        _write_gatelog(
            self.log_dir, "prior-fail", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_FAIL, gate_version=self.gate_version,
            tree_sha=self.merge_tree(),
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(
            res.started, [], f"a FAIL-memoized branch must not be dispatched: {res.started}"
        )
        self.assertIn("staging/one", fleetd.branch_names(res.needs_author))
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
        """A local gatelogs JSON says PASS for this branch's real merge
        tree, but the SHARED verdict cache holds a FAIL at that identical
        (tree_sha, gate_version, platform_id) key -- e.g. another host's
        more complete run. The hub's answer must win over the local
        recall."""
        tree_sha = self.merge_tree()
        _write_gatelog(
            self.log_dir, "prior-pass-stale", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_PASS, gate_version=self.gate_version,
            tree_sha=tree_sha, platform_id=self.platform_id,
        )
        self.assertEqual(self.store_verdict(verdict.RESULT_FAIL, tree_sha), "created")

        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(
            res.started, [], f"the shared hub FAIL must win over the stale local PASS: {res.started}"
        )
        self.assertIn("staging/one", fleetd.branch_names(res.needs_author))
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


# ----------------------------------------------------------------------- #
# R4's shared-cache clause -- hub-first selection, sha-keyed FAIL
# ----------------------------------------------------------------------- #


class TestHubFirstSelection(VerdictSelectionBase):
    """(a) Gate selection asks the SHARED verdict cache directly.

    The defect these pin: the selection path used to reach the hub only
    THROUGH a `~/gatelogs` hit, so a host that had never itself gated a
    branch never consulted the hub at all and re-bought a gate a peer had
    already paid for and published. `dispatch.merge_tree_sha` +
    `verdict.lookup` -- the same composition R5 already uses for agent
    dispatch -- is all it takes to ask honestly.
    """

    def test_peer_pass_parks_the_branch_with_no_local_gatelogs_at_all(self):
        tree = self.merge_tree()
        sha = self.branch_sha()
        self.assertEqual(self.store_verdict(verdict.RESULT_PASS, tree), "created")
        self.assertFalse(self.log_dir.exists(), "this host must have no gate history")

        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(
            res.started, [],
            f"a peer's PASS at this host's own gate.sh cache key must stop the "
            f"dispatch; gate.sh would re-derive that key and exit a cache hit: {res.started}",
        )
        self.assertEqual(res.needs_author, [])
        self.assertEqual(len(res.awaiting_train), 1, res.awaiting_train)
        entry = res.awaiting_train[0]
        self.assertEqual(entry["branch"], "staging/one")
        self.assertEqual(entry["sha"], sha)
        self.assertEqual(entry["source"], fleetd.SOURCE_HUB)

    def test_peer_fail_parks_the_branch_with_no_local_gatelogs_at_all(self):
        self.assertEqual(self.store_verdict(verdict.RESULT_FAIL, self.merge_tree()), "created")
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(res.started, [], f"a peer's FAIL must stop the dispatch: {res.started}")
        self.assertEqual(fleetd.branch_names(res.needs_author), ["staging/one"])
        self.assertEqual(res.needs_author[0]["source"], fleetd.SOURCE_HUB)

    def test_a_verdict_from_another_platform_does_not_park(self):
        """The key is EXACT, and deliberately so. A verdict carrying a
        different `platform_id` would NOT short-circuit this host's
        `gate.sh`, so gating here still buys a real, missing verdict.
        Honouring it would collapse the two toolchain identities
        `verdict.py`'s docstring says cost a day."""
        self.assertEqual(
            self.store_verdict(verdict.RESULT_PASS, self.merge_tree(),
                               platform_id="a-different-platform"),
            "created",
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        self.assertEqual(res.awaiting_train, [])
        self.assertEqual(res.needs_author, [])

    def test_a_verdict_from_another_gate_version_does_not_park(self):
        self.assertEqual(
            self.store_verdict(verdict.RESULT_PASS, self.merge_tree(),
                               gate_version=f"{self.gate_version}-stale"),
            "created",
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        self.assertEqual(res.awaiting_train, [])

    def test_a_cached_abort_does_not_park(self):
        """`verdict.lookup` never serves an ABORT as a settled answer, and
        selection must inherit that: an aborted tree schedules a retry."""
        self.assertEqual(self.store_verdict(verdict.RESULT_ABORT, self.merge_tree()), "created")
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        self.assertEqual(res.awaiting_train, [])
        self.assertEqual(res.needs_author, [])

    def test_an_unreachable_hub_never_blocks_dispatch(self):
        """Fail OPEN. Refusing to gate because a probe could not answer
        would idle the fleet for infrastructure reasons -- the mistake
        `dispatch.economic_refusal` and `_limits_ok` both name."""
        self.assertEqual(self.store_verdict(verdict.RESULT_PASS, self.merge_tree()), "created")
        real_lookup = verdict.lookup

        def exploding(hub, *a, **kw):
            raise HubUnreachableError("ls-remote failed: transient")

        verdict.lookup = exploding
        try:
            self.set_desired(gates=1)
            res = self.reconcile()
        finally:
            verdict.lookup = real_lookup
        self.assertEqual(len(res.started), 1,
                         f"a hub hiccup must fail open, not park: refused={res.refused}")

    def test_merge_tree_is_computed_once_per_candidate_and_then_memoized(self):
        """The bound R4's clause asks for: at most one `merge-tree` per
        candidate per loop, memoized by (branch_sha, tip_sha) so a second
        loop over an unchanged pair costs nothing."""
        self.assertEqual(self.store_verdict(verdict.RESULT_PASS, self.merge_tree()), "created")
        fleetd._MERGE_TREE_MEMO.clear()

        calls = []
        real = dispatch.merge_tree_sha

        def counting(hub, tip_sha, branch_sha):
            calls.append((tip_sha, branch_sha))
            return real(hub, tip_sha, branch_sha)

        fleetd.dispatch_mod.merge_tree_sha = counting
        try:
            self.set_desired(gates=1)
            first = self.reconcile()
            second = self.reconcile()
        finally:
            fleetd.dispatch_mod.merge_tree_sha = real

        self.assertEqual(first.started, [], f"setup: branch should be parked, not started")
        self.assertEqual(second.started, [])
        self.assertEqual(
            len(calls), 1,
            f"one candidate over two loops must cost exactly one merge-tree; got {calls}",
        )


    def test_a_candidate_with_no_cached_verdict_costs_no_hub_read(self):
        """`Hub.read` is a fetch -- one ssh round trip each -- so asking
        the cache per candidate would put a handshake on every queued
        branch every fifteen seconds. One `ls-remote` of the verdicts
        namespace answers "is anything cached at this key" for the whole
        loop, and only keys that exist are then read. The branch we are
        about to gate is by definition one with nothing cached, so it must
        cost no read at all."""
        reads = []
        real_read = self.hub.read

        def counting(ref):
            reads.append(ref)
            return real_read(ref)

        self.hub.read = counting
        try:
            self.set_desired(gates=1)
            res = self.reconcile()
        finally:
            self.hub.read = real_read
        self.assertEqual(len(res.started), 1, f"refused={res.refused}")
        self.assertEqual(
            [r for r in reads if r.startswith(dispatch.VERDICTS_PREFIX)], [],
            "an uncached candidate must not cost a verdict read",
        )

    def test_a_cached_candidate_costs_exactly_one_verdict_read(self):
        self.assertEqual(self.store_verdict(verdict.RESULT_PASS, self.merge_tree()), "created")
        reads = []
        real_read = self.hub.read

        def counting(ref):
            reads.append(ref)
            return real_read(ref)

        self.hub.read = counting
        try:
            self.set_desired(gates=1)
            res = self.reconcile()
        finally:
            self.hub.read = real_read
        self.assertEqual(res.started, [])
        self.assertEqual(
            len([r for r in reads if r.startswith(dispatch.VERDICTS_PREFIX)]), 1, reads
        )


class TestNeedsAuthorIsNotAPermanentLockout(VerdictSelectionBase):
    """(b) A FAIL is keyed to the SHA it was measured at.

    The defect these pin: the memo was keyed `(branch, base_tip)` -- the
    branch's NAME and the tip -- and a force-push changes neither. So the
    one action NEEDS_AUTHOR asks the author for made no difference, and the
    only event that could have replaced the memo was a gate of the branch,
    which the memo itself prevented. That is a deadlock, not a delay.
    """

    def test_a_force_pushed_branch_is_offered_again_local_recall(self):
        _write_gatelog(
            self.log_dir, "prior-fail", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_FAIL, gate_version=self.gate_version,
            tree_sha=self.merge_tree(), platform_id=self.platform_id,
        )
        self.set_desired(gates=1)
        before = self.reconcile()
        self.assertEqual(fleetd.branch_names(before.needs_author), ["staging/one"],
                         "setup: the unmoved branch must be parked first")

        new_sha = self.force_push("author-fixes-it")
        after = self.reconcile()
        self.assertEqual(
            fleetd.branch_names(after.needs_author), [],
            "a force-push is the author acting; the branch must stop being condemned",
        )
        self.assertEqual(len(after.started), 1, f"refused={after.refused}")
        self.assertNotEqual(new_sha, before.needs_author[0]["sha"])

    def test_a_force_pushed_branch_is_offered_again_hub_verdict(self):
        self.assertEqual(self.store_verdict(verdict.RESULT_FAIL, self.merge_tree()), "created")
        self.set_desired(gates=1)
        before = self.reconcile()
        self.assertEqual(fleetd.branch_names(before.needs_author), ["staging/one"])

        self.force_push("author-fixes-it")
        after = self.reconcile()
        self.assertEqual(after.needs_author, [],
                         "the hub's FAIL was measured at a tree the new sha no longer produces")
        self.assertEqual(len(after.started), 1, f"refused={after.refused}")

    def test_stale_fail_tree_sha_cannot_be_reconfirmed_onto_the_new_sha(self):
        """The reconfirmation path (`_confirm_via_shared_cache`) looks the
        RECALLED tree up on the hub. With a stale memo entry and a matching
        FAIL sitting in the shared cache at that OLD tree, both halves
        agree -- and both are about a sha that is no longer on the branch.
        Neither may resurrect the verdict."""
        old_tree = self.merge_tree()
        _write_gatelog(
            self.log_dir, "prior-fail", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_FAIL, gate_version=self.gate_version,
            tree_sha=old_tree, platform_id=self.platform_id,
        )
        self.assertEqual(self.store_verdict(verdict.RESULT_FAIL, old_tree), "created")

        self.set_desired(gates=1)
        self.assertEqual(fleetd.branch_names(self.reconcile().needs_author), ["staging/one"])

        self.force_push("author-fixes-it")
        after = self.reconcile()
        self.assertEqual(
            after.needs_author, [],
            "a verdict at the old tree says nothing about the new sha, from either source",
        )
        self.assertEqual(len(after.started), 1, f"refused={after.refused}")

    def test_a_hub_override_of_a_local_pass_is_also_sha_keyed(self):
        """The override path (local recall PASS, shared cache FAIL at that
        same recalled tree) produces a FAIL like any other, and must be
        held to the same proof: after a force-push the recalled tree is not
        the tree the new sha produces, so neither half may condemn it."""
        old_tree = self.merge_tree()
        _write_gatelog(
            self.log_dir, "prior-pass", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_PASS, gate_version=self.gate_version,
            tree_sha=old_tree, platform_id=self.platform_id,
        )
        self.assertEqual(self.store_verdict(verdict.RESULT_FAIL, old_tree), "created")
        self.set_desired(gates=1)
        self.assertEqual(fleetd.branch_names(self.reconcile().needs_author), ["staging/one"],
                         "setup: the hub FAIL must win over the local PASS while nothing moves")

        self.force_push("author-fixes-it")
        after = self.reconcile()
        self.assertEqual(after.needs_author, [])
        self.assertEqual(len(after.started), 1, f"refused={after.refused}")

    def test_an_unmoved_branch_stays_parked_across_loops(self):
        """The negative control: sha-keying must not have turned the FAIL
        memo into a no-op. Nothing moves, so nothing is re-offered."""
        _write_gatelog(
            self.log_dir, "prior-fail", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_FAIL, gate_version=self.gate_version,
            tree_sha=self.merge_tree(), platform_id=self.platform_id,
        )
        self.set_desired(gates=1)
        for loop in range(3):
            res = self.reconcile()
            self.assertEqual(res.started, [], f"loop {loop}: {res.started}")
            self.assertEqual(fleetd.branch_names(res.needs_author), ["staging/one"])

    def test_a_stale_local_pass_still_parks_without_a_confirmable_tree(self):
        """Asymmetry, on purpose. A PASS needs no sha proof to be SAFE: an
        unconfirmable PASS parks a branch for one more loop and the next
        hub-first lookup decides it properly. A FAIL condemns until a human
        acts, so only a FAIL has to prove it is about the current sha."""
        _write_gatelog(
            self.log_dir, "prior-pass", branch="staging/one", base_tip=self.tip_sha,
            result=verdict.RESULT_PASS, gate_version=self.gate_version,
            tree_sha="d" * 40, platform_id=self.platform_id,
        )
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertEqual(res.started, [])
        self.assertEqual(fleetd.branch_names(res.awaiting_train), ["staging/one"])


class TestParkedBranchesCarryTheirSha(VerdictSelectionBase):
    """(c) The heartbeat, and `fleet status`, show WHEN a parked verdict
    was decided -- a count cannot tell an operator whether the branch it
    counts is still the branch that was judged."""

    def test_heartbeat_entries_carry_branch_sha_and_source(self):
        sha = self.branch_sha()
        self.assertEqual(self.store_verdict(verdict.RESULT_PASS, self.merge_tree()), "created")
        self.set_desired(gates=1)
        res = self.reconcile()
        self.assertTrue(res.heartbeat_written)

        hb = self.hub.read(fleetd.HOSTS_PREFIX + self.host)
        self.assertEqual(hb["awaiting_train"], [
            {"branch": "staging/one", "sha": sha, "source": fleetd.SOURCE_HUB}
        ])

    def test_status_renders_a_parked_table_with_the_deciding_sha(self):
        sha = self.branch_sha()
        self.assertEqual(self.store_verdict(verdict.RESULT_FAIL, self.merge_tree()), "created")
        self.set_desired(gates=1)
        self.reconcile()

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        out = buf.getvalue()
        self.assertIn("PARKED", out)
        self.assertIn("DECIDED_AT", out)
        self.assertIn(sha[:12], out, f"the deciding sha must be visible:\n{out}")
        self.assertIn("NEEDS_AUTH", out)

        # and the per-host table above it is unchanged in shape
        header, rows = _parse_table(out)
        self.assertIn("NEEDS_AUTH", header)
        matching = [r for r in rows if r and r[0] == self.host]
        self.assertEqual(len(matching), 1, f"rows: {rows}")

    def test_status_tolerates_a_legacy_heartbeat_of_bare_names(self):
        """A mixed-version fleet has both shapes on the hub at once. The
        older one has no sha to show, and must render rather than crash or
        vanish from the counts."""
        hb = {
            "gates_running": 0, "agents_running": 0, "free_gb": 10.0, "free_mem_gb": 10.0,
            "rustc_id": "r", "platform_id": "p", "owning_user": "t", "oracle_ok": None,
            "gate_version": "4", "tip_generation_seen": None,
            "awaiting_train": ["staging/legacy-a"],
            "needs_author": ["staging/legacy-b"],
            "killed_this_loop": 0,
            "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        ref = fleetd.HOSTS_PREFIX + "oldhost"
        self.assertTrue(self.hub.create(ref, hb))

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        out = buf.getvalue()
        self.assertIn("staging/legacy-a", out)
        self.assertIn("staging/legacy-b", out)

    def test_branch_names_accepts_both_shapes(self):
        self.assertEqual(
            fleetd.branch_names([{"branch": "a", "sha": "x", "source": "hub"}, "b", None, {}]),
            ["a", "b"],
        )
        self.assertEqual(fleetd.branch_names(None), [])



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

    def test_status_why_renders_refused_reasons_heartbeat_age_and_desired(self):
        """PLAN Stage 1 task 5: `fleet status --why` must answer "why is
        nothing starting" per host from the durable heartbeat's `refused`
        field plus the desired targets -- no ssh, no re-derivation."""
        self.set_desired(gates=3, enabled=False, reason="quarantine test")
        hb = {
            "gates_running": 0, "agents_running": 0, "free_gb": 10.0, "free_mem_gb": 10.0,
            "rustc_id": "r", "platform_id": "p", "owning_user": "t", "oracle_ok": None,
            "gate_version": "4", "tip_generation_seen": None,
            "refused": [["disabled", "quarantine test"],
                        ["limits", "low-disk 5G < floor 14G"]],
            "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        self.assertTrue(self.hub.create(fleetd.HOSTS_PREFIX + self.host, hb))

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = cli.main(["--hub", str(self.bare), "status", "--why"])
        self.assertEqual(rc, 0)
        out = buf.getvalue()

        self.assertIn("WHY", out)
        why = out[out.index("WHY"):]
        self.assertIn(self.host, why)
        # heartbeat age: written seconds ago, must render as an elapsed
        # count, never "never" (that is the DOWN/no-heartbeat case).
        self.assertRegex(why, r"heartbeat age \d+s")
        # desired targets from `refs/fleet/desired`, not from the heartbeat.
        self.assertIn("desired gates=3 agents=0", why)
        # both refused reasons, with their detail.
        self.assertIn("refused: disabled (quarantine test)", why)
        self.assertIn("refused: limits (low-disk 5G < floor 14G)", why)

    def test_status_why_reports_no_refused_reasons_when_list_is_empty(self):
        self.set_desired(gates=1)
        hb = {
            "gates_running": 0, "agents_running": 0, "free_gb": 10.0, "free_mem_gb": 10.0,
            "rustc_id": "r", "platform_id": "p", "owning_user": "t", "oracle_ok": None,
            "gate_version": "4", "tip_generation_seen": None, "refused": [],
            "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        self.assertTrue(self.hub.create(fleetd.HOSTS_PREFIX + self.host, hb))

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status", "--why"])
        why = buf.getvalue()
        self.assertIn("(no refused reasons on file)", why)

    def test_status_without_why_flag_omits_the_why_section(self):
        self.set_desired(gates=1, enabled=False, reason="quarantine test")
        hb = {
            "gates_running": 0, "agents_running": 0, "free_gb": 10.0, "free_mem_gb": 10.0,
            "rustc_id": "r", "platform_id": "p", "owning_user": "t", "oracle_ok": None,
            "gate_version": "4", "tip_generation_seen": None,
            "refused": [["disabled", "quarantine test"]],
            "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        self.assertTrue(self.hub.create(fleetd.HOSTS_PREFIX + self.host, hb))

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        self.assertNotIn("WHY", buf.getvalue())

    def test_status_work_column_is_dash_with_no_live_claims(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            cli.main(["--hub", str(self.bare), "status"])
        out = buf.getvalue()
        self.assertIn("(no host heartbeats yet", out)


if __name__ == "__main__":
    unittest.main()
