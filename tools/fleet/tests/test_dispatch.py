#!/usr/bin/env python3
"""Dispatch economics: every agent run is paid (ARCH-FIX-SPEC.md R5).

Instrument: plain `unittest` against a throwaway fixture hub under
`tempfile.gettempdir()`. No real hub, no real coding CLI (the agent is a
stub script), no Rust built. The one thing these tests deliberately do NOT
mock is the git object store: drift and merge-tree questions are answered
by real `git merge-base` / `git merge-tree` against real commits, because a
mocked answer to "does this branch contain the tip" would test the mock.

Two tests here spawn genuinely fresh `python3` processes
(`test_count_survives_a_fresh_process`, and the restart half of
`test_cooldown_is_durable_across_a_restart`). That is not ceremony: the
defect R5 names is precisely that the previous implementation's state lived
in a dict which every restart emptied, and a same-process test of a
same-process dict passes just as green for the broken version as for the
fixed one. The instrument has to be a new process or it is not measuring
the property.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import dispatch  # noqa: E402
import fleetd  # noqa: E402
import verdict  # noqa: E402
from fleetlib import Hub  # noqa: E402
from _env import HermeticCase, scrub_env  # noqa: E402

TIP_REF = "refs/heads/refactor/tag-machinery"
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}


# --------------------------------------------------------------------- #
# Fixture
# --------------------------------------------------------------------- #


class Fixture:
    """A bare hub plus a seed worktree, with helpers to build the exact
    ancestry each test needs (drifted / merged / already-contains-tip)."""

    def __init__(self, tmp: Path):
        assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
        self.tmp = tmp
        self.bare = tmp / "hub.git"
        self.work = tmp / "seed"
        self.env = scrub_env(**GIT_ENV)
        subprocess.run(["git", "init", "-q", "--bare", str(self.bare)], check=True)
        subprocess.run(["git", "init", "-q", str(self.work)], check=True)
        self.base = self._commit("base", "f.txt", "base\n")
        self._push(f"HEAD:{TIP_REF}")
        self.tip = self.base

    # -- git helpers -------------------------------------------------- #

    def _git(self, *args, check=True):
        return subprocess.run(["git", "-C", str(self.work), *args],
                              check=check, capture_output=True, text=True, env=self.env)

    def _commit(self, msg: str, path: str, content: str) -> str:
        (self.work / path).write_text(content)
        self._git("add", ".")
        self._git("commit", "-qm", msg)
        return self._git("rev-parse", "HEAD").stdout.strip()

    def _push(self, refspec: str, force: bool = False):
        args = ["push", "-q"] + (["-f"] if force else []) + [str(self.bare), refspec]
        self._git(*args)

    # -- scenario builders -------------------------------------------- #

    def advance_tip(self, marker: str = "moved") -> str:
        """Move the tip forward from its current position."""
        self._git("checkout", "-q", "-B", "tipwork", self.tip)
        self.tip = self._commit(f"tip {marker}", f"tip-{marker}.txt", marker + "\n")
        self._push(f"HEAD:{TIP_REF}", force=True)
        return self.tip

    def drifted_branch(self, slug: str, base: str = None) -> str:
        """A staging branch off `base` (default: the ORIGINAL base, so it
        does not contain a tip that has since moved) -- i.e. real drift."""
        self._git("checkout", "-q", "-B", f"w-{slug}", base or self.base)
        sha = self._commit(f"work {slug}", f"{slug}.txt", slug + "\n")
        self._push(f"HEAD:refs/heads/staging/{slug}")
        return sha

    def branch_containing_tip(self, slug: str) -> str:
        """A staging branch that has already merged the current tip: no
        drift left, so a convergence agent would have nothing to do."""
        self._git("checkout", "-q", "-B", f"w-{slug}", self.base)
        self._commit(f"work {slug}", f"{slug}.txt", slug + "\n")
        self._git("merge", "-q", "--no-edit", self.tip)
        sha = self._git("rev-parse", "HEAD").stdout.strip()
        self._push(f"HEAD:refs/heads/staging/{slug}")
        return sha

    def merged_branch(self, slug: str) -> str:
        """A staging branch that IS an ancestor of the tip."""
        self._push(f"{self.tip}:refs/heads/staging/{slug}")
        return self.tip

    def hub(self, name: str = "cache") -> Hub:
        return Hub(str(self.bare), workdir=self.tmp / name)


class DispatchBase(HermeticCase):
    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        self.fx = Fixture(self.tmp)
        self.hub = self.fx.hub()
        self.host = "testhost"
        self._saved = (dispatch.MAX_ATTEMPTS, dispatch.COOLDOWN_S)

    def tearDown(self):
        dispatch.MAX_ATTEMPTS, dispatch.COOLDOWN_S = self._saved
        self.tmpdir.cleanup()

    def in_fresh_process(self, snippet: str) -> str:
        """Run `snippet` in a brand-new interpreter against the same hub.

        The whole point of the durable ledger is that a process which has
        never seen the previous one's memory still knows what was bought.
        """
        code = (
            "import sys; sys.path.insert(0, %r)\n"
            "from fleetlib import Hub; import dispatch\n"
            "hub = Hub(%r, workdir=%r)\n" % (str(FLEET_DIR), str(self.fx.bare), str(self.tmp / "freshcache"))
        ) + snippet
        r = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, timeout=120)
        self.assertEqual(r.returncode, 0, f"fresh process failed:\n{r.stdout}\n{r.stderr}")
        return r.stdout.strip()


# --------------------------------------------------------------------- #
# R5.2 -- the durable attempt ledger
# --------------------------------------------------------------------- #


class TestDurableAttempts(DispatchBase):
    def test_count_survives_a_fresh_process(self):
        """THE R5.2 property: a restart must not reset counts.

        Instrument: a separate `python3 -c` that shares nothing with this
        interpreter but the hub URL. Under the old in-memory
        `fleetd._agent_attempts` this test could not even be written.
        """
        for _ in range(2):
            dispatch.record_dispatch(self.hub, "staging/x", self.host)

        out = self.in_fresh_process(
            "print(dispatch.load(hub, 'staging/x')['count'])"
        )
        self.assertEqual(out, "2", "a fresh process must see both paid runs")

    def test_the_in_memory_attempt_dict_is_gone(self):
        """The dict this replaced must not survive alongside the ledger --
        two sources of truth for 'what has been bought' is the defect, not
        the fix."""
        self.assertFalse(
            hasattr(fleetd, "_agent_attempts"),
            "fleetd._agent_attempts still exists; the durable ledger has a rival",
        )
        self.assertFalse(hasattr(fleetd, "AGENT_RETRY_COOLDOWN_S"))

    def test_ledger_ref_shape_and_payload(self):
        rec = dispatch.record_dispatch(self.hub, "staging/foo", self.host)
        self.assertEqual(dispatch.attempt_ref("staging/foo"),
                         "refs/fleet/attempts/staging-foo")
        on_hub = self.hub.read("refs/fleet/attempts/staging-foo")
        for field in ("count", "last_at", "last_outcome", "last_host"):
            self.assertIn(field, on_hub, f"payload must carry {field}")
        self.assertEqual(on_hub["count"], 1)
        self.assertEqual(on_hub["last_outcome"], "dispatched")
        self.assertEqual(on_hub["last_host"], self.host)
        self.assertEqual(rec["count"], 1)

    def test_attempt_key_matches_the_agent_claim_key(self):
        """One key, two namespaces (dispatch.attempt_key's docstring)."""
        for branch in ("staging/foo", "intent:bar", "staging/a/b"):
            self.assertEqual(
                dispatch.attempt_key(branch),
                branch.replace("/", "-").replace(":", "-"),
                "attempts key must match the claim key fleetd.start_agent builds",
            )

    def test_hard_cap_at_three(self):
        dispatch.COOLDOWN_S = 0  # isolate the cap from the cooldown
        for i in range(3):
            self.assertIsNone(
                dispatch.budget_refusal(dispatch.load(self.hub, "staging/x")),
                f"dispatch {i + 1} of 3 must be allowed",
            )
            dispatch.record_dispatch(self.hub, "staging/x", self.host)
            dispatch.record_outcome(self.hub, "staging/x", self.host, "no-progress")

        refusal = dispatch.budget_refusal(dispatch.load(self.hub, "staging/x"))
        self.assertIsNotNone(refusal, "the 4th dispatch must be refused")
        self.assertEqual(refusal[0], "attempt-cap")
        self.assertIn("3/3", refusal[1])

    def test_cap_and_cooldown_are_env_overridable(self):
        """R5: 'a hard cap (3)' -- env-overridable, read at import."""
        probe = "import sys; sys.path.insert(0, %r); import dispatch; print(dispatch.MAX_ATTEMPTS, dispatch.COOLDOWN_S)" % str(FLEET_DIR)
        r = subprocess.run(
            [sys.executable, "-c", probe], capture_output=True, text=True, timeout=60,
            env=scrub_env(FLEET_AGENT_MAX_ATTEMPTS="7", FLEET_AGENT_COOLDOWN_S="12"),
        )
        self.assertEqual(r.stdout.strip(), "7 12.0", r.stderr)

        r_default = subprocess.run(
            [sys.executable, "-c", probe], capture_output=True, text=True, timeout=60,
            env=scrub_env(),  # drops those two, and every other FLEET_* the invoker set
        )
        self.assertEqual(r_default.stdout.strip(), "3 1800.0", r_default.stderr)

    def test_progress_resets_the_consecutive_count(self):
        """The cap counts CONSECUTIVE failures. A branch that converged has
        not used up its three chances for the next time it goes stale."""
        for _ in range(2):
            dispatch.record_dispatch(self.hub, "staging/x", self.host)
            dispatch.record_outcome(self.hub, "staging/x", self.host, "no-progress")
        self.assertEqual(dispatch.load(self.hub, "staging/x")["count"], 2)

        dispatch.record_dispatch(self.hub, "staging/x", self.host)
        dispatch.record_outcome(self.hub, "staging/x", self.host, "converged")
        self.assertEqual(dispatch.load(self.hub, "staging/x")["count"], 0)

    def test_blocked_does_not_reset_the_count(self):
        """BLOCKED is a paid run that made no progress. If it reset the
        count, a branch no agent can solve would be re-bought forever --
        the cap would never bind on the one case it exists for."""
        for _ in range(3):
            dispatch.record_dispatch(self.hub, "staging/x", self.host)
            dispatch.record_outcome(self.hub, "staging/x", self.host, "blocked")
        self.assertEqual(dispatch.load(self.hub, "staging/x")["count"], 3)
        self.assertEqual(
            dispatch.budget_refusal(dispatch.load(self.hub, "staging/x"))[0],
            "attempt-cap",
        )

    def test_not_paid_hands_the_attempt_back(self):
        dispatch.record_dispatch(self.hub, "staging/x", self.host)
        dispatch.record_dispatch(self.hub, "staging/x", self.host)
        dispatch.record_outcome(self.hub, "staging/x", self.host, dispatch.NOT_PAID)
        self.assertEqual(dispatch.load(self.hub, "staging/x")["count"], 1)

    def test_cooldown_is_durable_across_a_restart(self):
        """The cooldown is derived from the ref's `last_at`, so it is the
        same answer in a process that has been up for a week and one that
        started a millisecond ago."""
        dispatch.record_dispatch(self.hub, "staging/x", self.host)
        out = self.in_fresh_process(
            "r = dispatch.load(hub, 'staging/x')\n"
            "print(dispatch.budget_refusal(r, cooldown_s=1800)[0])"
        )
        self.assertEqual(out, "cooldown", "a fresh process must honour the cooldown")

        # ...and it lapses on the record's own timestamp, not on uptime.
        stale = dispatch._iso(datetime.now(timezone.utc) - timedelta(seconds=4000))
        ref = dispatch.attempt_ref("staging/x")
        payload = self.hub.read(ref)
        payload["last_at"] = stale
        self.assertTrue(self.hub.update(ref, payload, self.hub.sha(ref)))
        self.assertIsNone(
            dispatch.budget_refusal(dispatch.load(self.hub, "staging/x"), cooldown_s=1800)
        )

    def test_concurrent_records_do_not_lose_a_count(self):
        """Two writers racing the same key: the CAS loop must serialize
        them, not let one clobber the other's count."""
        second = self.fx.hub("cache2")
        dispatch.record_dispatch(self.hub, "staging/x", "host-a")
        dispatch.record_dispatch(second, "staging/x", "host-b")
        self.assertEqual(dispatch.load(self.hub, "staging/x")["count"], 2)
        self.assertEqual(dispatch.load(self.hub, "staging/x")["last_host"], "host-b")

    def test_clear_gives_the_branch_its_chances_back(self):
        dispatch.record_dispatch(self.hub, "staging/x", self.host)
        self.assertTrue(dispatch.clear(self.hub, "staging/x"))
        self.assertEqual(dispatch.load(self.hub, "staging/x")["count"], 0)
        self.assertTrue(dispatch.clear(self.hub, "staging/never-existed"))

    def test_malformed_record_degrades_instead_of_crashing(self):
        ref = dispatch.attempt_ref("staging/x")
        self.assertTrue(self.hub.create(ref, {"count": "banana", "last_at": "not-a-date"}))
        record = dispatch.load(self.hub, "staging/x")
        self.assertEqual(record["count"], 0)
        self.assertIsNone(dispatch.budget_refusal(record))


# --------------------------------------------------------------------- #
# R5.1 -- economic preflight
# --------------------------------------------------------------------- #


class TestEconomics(DispatchBase):
    def test_drifted_branch_is_worth_converging(self):
        self.fx.drifted_branch("drift")
        self.fx.advance_tip()
        self.assertIsNone(
            dispatch.economic_refusal(self.hub, "staging/drift", self.fx.tip),
            "a branch behind a moved tip is exactly what convergence is for",
        )

    def test_branch_already_containing_the_tip_is_refused(self):
        """R5: 'convergence requires drift (merge-base != tip)'."""
        self.fx.advance_tip()
        self.fx.branch_containing_tip("fresh")
        refusal = dispatch.economic_refusal(self.hub, "staging/fresh", self.fx.tip)
        self.assertIsNotNone(refusal)
        self.assertEqual(refusal[0], "no-drift")

    def test_branch_already_merged_is_refused(self):
        self.fx.merged_branch("landed")
        refusal = dispatch.economic_refusal(self.hub, "staging/landed", self.fx.tip)
        self.assertIsNotNone(refusal)
        self.assertEqual(refusal[0], "already-merged")

    def test_cached_pass_for_the_pair_is_refused(self):
        """R5: 'any mode requires no cached PASS for (branch_sha, tip_sha)'.

        The verdict cache is keyed by the MERGE TREE, so the check computes
        the tree this pair would produce and looks for a PASS beneath it.
        """
        branch_sha = self.fx.drifted_branch("passing")
        self.fx.advance_tip()
        self.assertIsNone(
            dispatch.economic_refusal(self.hub, "staging/passing", self.fx.tip),
            "precondition: without a verdict this pair is worth a run",
        )

        tree = dispatch.merge_tree_sha(self.hub, self.fx.tip, branch_sha)
        self.assertIsNotNone(tree, "fixture merge must be clean")
        verdict.store(self.hub, {
            "tree_sha": tree, "base_tip": self.fx.tip, "branch": "staging/passing",
            "result": "PASS", "stage": "all", "gate_version": "4",
            "rustc_id": "r", "platform_id": "p", "host": "someotherhost",
            "duration_s": 1, "write_set": [],
        })

        refusal = dispatch.economic_refusal(self.hub, "staging/passing", self.fx.tip)
        self.assertIsNotNone(refusal, "a pair that already PASSed is awaiting the train")
        self.assertEqual(refusal[0], "cached-pass")

    def test_cached_fail_does_not_refuse(self):
        """A FAIL is a reason to send an agent, not a reason not to."""
        branch_sha = self.fx.drifted_branch("failing")
        self.fx.advance_tip()
        # merge_tree_sha reads the local object cache; `economic_refusal`
        # fetches into it first, so a direct caller must too.
        self.assertTrue(dispatch._have_objects(
            self.hub, [TIP_REF, "refs/heads/staging/failing"]))
        tree = dispatch.merge_tree_sha(self.hub, self.fx.tip, branch_sha)
        self.assertIsNotNone(tree, "fixture merge must be clean")
        verdict.store(self.hub, {
            "tree_sha": tree, "base_tip": self.fx.tip, "branch": "staging/failing",
            "result": "FAIL", "stage": "clippy", "gate_version": "4",
            "rustc_id": "r", "platform_id": "p", "host": "h",
            "duration_s": 1, "write_set": [],
        })
        self.assertIsNone(dispatch.economic_refusal(self.hub, "staging/failing", self.fx.tip))

    def test_authoring_keys_are_exempt_from_the_drift_test(self):
        """There is no branch yet, so there is no merge-base to compare."""
        self.assertIsNone(
            dispatch.economic_refusal(self.hub, "intent:brand-new", self.fx.tip)
        )

    def test_absent_branch_is_refused(self):
        refusal = dispatch.economic_refusal(self.hub, "staging/ghost", self.fx.tip)
        self.assertEqual(refusal[0], "no-such-branch")

    def test_unanswerable_probe_fails_open(self):
        """A fetch we cannot perform must not idle the fleet -- the same
        rule `_limits_ok` applies to an unavailable memory probe."""
        self.fx.drifted_branch("x")
        self.fx.advance_tip()
        broken = Hub(str(self.fx.bare), workdir=self.tmp / "brokencache")
        real_sha = self.hub.sha("refs/heads/staging/x")
        original = dispatch._have_objects
        dispatch._have_objects = lambda *a, **k: False
        try:
            self.assertIsNone(
                dispatch.economic_refusal(broken, "staging/x", self.fx.tip, branch_sha=real_sha)
            )
        finally:
            dispatch._have_objects = original


# --------------------------------------------------------------------- #
# R5.3 -- the reserved authoring slot
# --------------------------------------------------------------------- #


class TestReservedAuthoringSlot(HermeticCase):
    """Unit-level: the ordering policy itself, independent of any hub."""

    @staticmethod
    def _records(newest_branch: str) -> dict:
        now = datetime.now(timezone.utc)
        return {
            "old": {"branch": "staging/old", "last_at": dispatch._iso(now - timedelta(hours=2))},
            "new": {"branch": newest_branch, "last_at": dispatch._iso(now)},
        }

    def test_authoring_wins_when_the_last_dispatch_was_convergence(self):
        order = dispatch.order_candidates(
            ["staging/a", "staging/b"], ["intent:i"], self._records("staging/old")
        )
        self.assertEqual(order[0], "intent:i")

    def test_convergence_wins_when_the_last_dispatch_was_authoring(self):
        order = dispatch.order_candidates(
            ["staging/a", "staging/b"], ["intent:i"], self._records("intent:prev")
        )
        self.assertEqual(order[0], "staging/a")

    def test_two_slots_serve_both_kinds_in_one_round(self):
        order = dispatch.order_candidates(
            ["staging/a", "staging/b", "staging/c"], ["intent:i"], self._records("staging/old")
        )
        self.assertEqual(set(order[:2]), {"intent:i", "staging/a"})

    def test_one_sided_inputs_pass_through(self):
        self.assertEqual(dispatch.order_candidates(["staging/a"], [], {}), ["staging/a"])
        self.assertEqual(dispatch.order_candidates([], ["intent:i"], {}), ["intent:i"])
        self.assertEqual(dispatch.order_candidates([], [], {}), [])

    def test_no_candidate_is_dropped_by_the_interleave(self):
        order = dispatch.order_candidates(
            ["c1", "c2", "c3"], ["a1", "a2"], self._records("staging/old")
        )
        self.assertEqual(sorted(order), ["a1", "a2", "c1", "c2", "c3"])

    def test_unparseable_timestamps_do_not_skew_the_alternation(self):
        records = {
            "junk": {"branch": "intent:junk", "last_at": "not-a-date"},
            "real": {"branch": "staging/real",
                     "last_at": dispatch._iso(datetime.now(timezone.utc))},
        }
        self.assertFalse(dispatch.last_dispatch_was_authoring(records))


# --------------------------------------------------------------------- #
# fleetd integration: the dispatch path end to end
# --------------------------------------------------------------------- #


def make_stub_cli(tmp: Path) -> Path:
    stub = tmp / "stub-cli.sh"
    stub.write_text("#!/bin/bash\necho 'stub agent ran'\nexit 0\n")
    stub.chmod(0o755)
    return stub


class FleetdDispatchBase(DispatchBase):
    def setUp(self):
        super().setUp()
        self.workers: list = []
        os.environ["FLEET_HOST"] = self.host
        os.environ["FLEET_AGENT_CLI_OVERRIDE"] = str(make_stub_cli(self.tmp))
        self.stub_gate = self.tmp / "stub-gate.sh"
        self.stub_gate.write_text("#!/bin/bash\nsleep 30\n")
        self.stub_gate.chmod(0o755)

    def tearDown(self):
        for w in self.workers:
            if w.popen is None:
                continue
            if w.kind == "gate":
                w.popen.kill()  # parked stub gate: nothing waits for it
            try:
                w.popen.wait(timeout=30)
            except subprocess.TimeoutExpired:
                w.popen.kill()
        os.environ.pop("FLEET_HOST", None)
        os.environ.pop("FLEET_AGENT_CLI_OVERRIDE", None)
        super().tearDown()

    def set_desired(self, gates=0, agents=0, enabled=True):
        doc = {"generation": 1,
               "hosts": {self.host: {"gates": gates, "agents": agents, "enabled": enabled}},
               "limits": {}}
        cur = self.hub.sha(fleetd.DESIRED_REF)
        ok = (self.hub.create(fleetd.DESIRED_REF, doc) if cur is None
              else self.hub.update(fleetd.DESIRED_REF, doc, cur))
        self.assertTrue(ok)

    def open_intent(self, slug: str, title: str = "do the thing"):
        self.assertTrue(self.hub.create(
            f"refs/fleet/intents/{slug}",
            {"status": "open", "title": title, "scope": {"formats": ["AIFF"]}},
        ))

    def reconcile(self):
        return fleetd.reconcile_once(
            self.hub, self.host, self.workers,
            gate_command=[str(self.stub_gate)],
            log_dir=self.tmp / "logs",
            repo_root=FLEET_DIR.parents[1],
            disk_probe=lambda: 100.0,
            mem_probe=lambda: 32.0,
        )


class TestFleetdDispatch(FleetdDispatchBase):
    def test_uneconomic_branch_is_never_spawned(self):
        """The refusal happens at fleetd, before a process exists -- the
        layer where it is cheapest. Evidence: no worker, no agent claim,
        and no ledger entry (nothing was bought, so nothing is counted)."""
        self.fx.advance_tip()
        self.fx.branch_containing_tip("nodrift")
        self.set_desired(agents=1)

        res = self.reconcile()
        self.assertEqual(res.started, [], "a no-drift branch must not be bought")
        self.assertIn("agent-no-drift", [r[0] for r in res.refused], res.refused)
        self.assertEqual(self.hub.list("refs/fleet/claims/agent/"), {},
                         "a refused dispatch must not take a claim")
        self.assertEqual(self.hub.list(dispatch.ATTEMPTS_PREFIX), {},
                         "a refused dispatch must not be counted as paid")

    def test_dispatch_is_counted_before_the_spawn(self):
        self.fx.drifted_branch("real")
        self.fx.advance_tip()
        self.set_desired(agents=1)

        res = self.reconcile()
        self.assertEqual(len(res.started), 1, res.refused)
        record = dispatch.load(self.hub, "staging/real")
        self.assertEqual(record["count"], 1)
        self.assertEqual(record["last_host"], self.host)

    def test_outcome_is_recorded_when_the_worker_is_reaped(self):
        self.fx.drifted_branch("real")
        self.fx.advance_tip()
        self.set_desired(agents=1)

        self.reconcile()
        self.assertEqual(len(self.workers), 1, "precondition: one agent running")
        self.workers[0].popen.wait(timeout=120)
        rc = self.workers[0].popen.returncode

        self.reconcile()
        record = dispatch.load(self.hub, "staging/real")
        self.assertIn(record["last_outcome"], fleetd._AGENT_RC_OUTCOMES.values(),
                      f"worker exited {rc}; ledger says {record['last_outcome']!r}")
        self.assertNotEqual(record["last_outcome"], "dispatched",
                            "the reap must close the ledger entry it opened")

    def test_a_started_gate_does_not_consume_the_agent_slots(self):
        """`reconcile_once` starts gates and agents in one step and shares
        one `res.started` list between them. Counting filled agent slots
        off that shared list lets a single started gate swallow every agent
        slot -- silently, since the refusal is simply an absence."""
        self.fx.drifted_branch("gateme")
        self.fx.advance_tip()
        self.open_intent("author-me")
        self.set_desired(gates=1, agents=1)

        res = self.reconcile()
        gates = [w for w in self.workers if w.kind == "gate"]
        agents = [w for w in self.workers if w.kind == "agent"]
        self.assertEqual(len(gates), 1, f"gate slot: {res.refused}")
        self.assertEqual(len(agents), 1,
                         f"agent slot must be filled independently: {res.refused}")
        self.assertEqual(agents[0].branch, "intent:author-me")

    def test_capped_branch_is_not_dispatched_again(self):
        self.fx.drifted_branch("stuck")
        self.fx.advance_tip()
        self.set_desired(agents=1)
        dispatch.COOLDOWN_S = 0
        for _ in range(dispatch.MAX_ATTEMPTS):
            dispatch.record_dispatch(self.hub, "staging/stuck", self.host)
            dispatch.record_outcome(self.hub, "staging/stuck", self.host, "no-progress")

        res = self.reconcile()
        self.assertEqual(res.started, [])
        self.assertIn("agent-attempt-cap", [r[0] for r in res.refused], res.refused)

    def test_authoring_is_not_starved_by_a_convergence_backlog(self):
        """THE R5.3 starvation test, stated by the requirement: a
        convergence backlog of 5 + 1 open intent + agents=1 must give the
        intent a slot at least every other dispatch.

        Before the reserved slot, `fleetd` reached the intent backlog only
        via `if not todo:` -- authoring was unreachable while ANY stale
        branch existed, so this test's expected count was zero, forever.
        """
        for i in range(5):
            self.fx.drifted_branch(f"b{i}")
        self.fx.advance_tip()
        self.open_intent("the-intent")
        self.set_desired(agents=1)
        # Cooldown off and cap lifted so the same candidates stay eligible
        # across rounds: this test is about ORDERING, and a cooldown would
        # retire each pick after one round and hide the alternation.
        dispatch.COOLDOWN_S = 0
        dispatch.MAX_ATTEMPTS = 99

        rounds = 6
        picks = []
        for i in range(rounds):
            res = self.reconcile()
            agents = [w for w in self.workers if w.kind == "agent"]
            self.assertEqual(len(agents), 1, f"round {i}: nothing dispatched ({res.refused})")
            picks.append(agents[0].branch)
            agents[0].popen.wait(timeout=120)

        authored = [i for i, p in enumerate(picks) if p.startswith(dispatch.INTENT_PREFIX)]
        self.assertGreaterEqual(
            len(authored), rounds // 2,
            f"the intent got {len(authored)}/{rounds} slots; picks={picks}",
        )
        self.assertEqual(picks[0], "intent:the-intent",
                         f"the first free slot must prefer authoring; picks={picks}")
        # No two consecutive convergence picks: that is what "at least
        # every other dispatch" means, checked directly rather than via the
        # count alone.
        for i in range(1, len(picks)):
            self.assertFalse(
                not picks[i].startswith(dispatch.INTENT_PREFIX)
                and not picks[i - 1].startswith(dispatch.INTENT_PREFIX),
                f"authoring skipped two dispatches in a row: picks={picks}",
            )

    def test_convergence_is_not_starved_by_authoring_either(self):
        """The alternation is symmetric -- a reserved slot that never gave
        the slot back would just move the starvation."""
        self.fx.drifted_branch("only-branch")
        self.fx.advance_tip()
        for i in range(5):
            self.open_intent(f"i{i}")
        self.set_desired(agents=1)
        dispatch.COOLDOWN_S = 0
        dispatch.MAX_ATTEMPTS = 99

        picks = []
        for _ in range(4):
            self.reconcile()
            agents = [w for w in self.workers if w.kind == "agent"]
            self.assertEqual(len(agents), 1)
            picks.append(agents[0].branch)
            agents[0].popen.wait(timeout=120)

        self.assertIn("staging/only-branch", picks,
                      f"convergence must still get a slot; picks={picks}")


# --------------------------------------------------------------------- #
# agentworker's own preflight
# --------------------------------------------------------------------- #


class TestAgentworkerPreflight(DispatchBase):
    def test_preflight_refuses_before_cloning_or_paying(self):
        """The worker's own check, invoked as a real subprocess with a stub
        CLI that would print a marker if it ever ran. Exit code 8, marker
        absent: no CLI was invoked, so nothing was paid."""
        self.fx.advance_tip()
        self.fx.branch_containing_tip("nodrift")
        marker = self.tmp / "cli-ran"
        stub = self.tmp / "loud-cli.sh"
        stub.write_text(f"#!/bin/bash\ntouch {marker}\nexit 0\n")
        stub.chmod(0o755)

        r = subprocess.run(
            [sys.executable, str(FLEET_DIR / "agentworker.py"),
             "--branch", "staging/nodrift", "--hub", str(self.fx.bare), "--host", self.host],
            capture_output=True, text=True, timeout=180,
            env=scrub_env(FLEET_AGENT_CLI_OVERRIDE=str(stub)),
        )
        self.assertEqual(r.returncode, 8, f"stdout={r.stdout}\nstderr={r.stderr}")
        self.assertIn("PREFLIGHT REFUSED", r.stdout)
        self.assertIn("no-drift", r.stdout)
        self.assertFalse(marker.exists(), "the CLI must never have been invoked")

    def test_preflight_passes_a_genuinely_stale_branch(self):
        self.fx.drifted_branch("stale")
        self.fx.advance_tip()
        import agentworker
        self.assertIsNone(
            agentworker.preflight(self.hub, "staging/stale", self.fx.tip)
        )

    def test_blocked_gets_its_own_exit_code(self):
        """BLOCKED must not share exit 0 with success -- fleetd's ledger
        resets the failure count on 0, and a permanently blocked branch
        would then be re-bought forever."""
        import agentworker
        self.assertEqual(agentworker.RC_BLOCKED, 9)
        self.assertEqual(agentworker.RC_PREFLIGHT_REFUSED, 8)
        self.assertEqual(fleetd._AGENT_RC_OUTCOMES[9], "blocked")
        self.assertEqual(fleetd._AGENT_RC_OUTCOMES[8], dispatch.NOT_PAID)
        self.assertNotIn("blocked", dispatch.PROGRESS_OUTCOMES)


if __name__ == "__main__":
    unittest.main()
