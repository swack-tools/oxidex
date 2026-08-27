#!/usr/bin/env python3
"""Unit tests for tools/fleet/rollout/rulesets.py.

Hermetic by construction: every test stubs `rulesets.gh_api`, the one
transport seam, so nothing here contacts GitHub. The LIVE half -- proving
GitHub actually rejects the pushes these rulesets describe -- is
`tools/fleet/tests/live/test_tip_ruleset.py`, opt-in behind
`FLEET_LIVE_GITHUB=1` and one directory down so `gate.sh`'s
`_fleet_test_modules()` glob (direct children of tools/fleet/tests only,
gate.sh L207-215) cannot pick it up.

What is worth testing here is not "does argparse work" but the three
properties whose violation is silent and expensive:

  1. A guard ruleset must have NO bypass actors. A bypass actor bypasses
     the whole ruleset it is attached to, so one bypass on `tip-guard`
     would hand the train `--force` and `--delete` on the tip -- exactly
     the finding (SPEC §11, J1 #4) that made this five rulesets and not two.
  2. `tip-guard` and `proof-guard` must carry the SAME rules. The live test
     proves the rule text on `keel-proof/*` and claims that proves it on the
     tip; that claim is only true while the two are built from one list.
  3. A restrict-updates ruleset must never be created with an unresolved
     bypass. `bypass_actors: []` on a `update` rule locks the ref against
     everyone including the train, and the fix requires a repo admin.

Run with:
    ( cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_rulesets )
"""

from __future__ import annotations

import contextlib
import io
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # tools/fleet

from rollout import rulesets  # noqa: E402
from _env import HermeticCase  # noqa: E402


REPO = "swack-tools/oxidex"


class StubGh:
    """Records every `gh api` call and answers from a canned world.

    Installed over `rulesets.gh_api`, which is the single function every
    other helper routes through -- so a test that forgets to stub something
    fails with a KeyError here rather than reaching the network.
    """

    def __init__(self, live_rulesets=None, deploy_keys=None):
        self.live = list(live_rulesets or [])
        self.keys = list(deploy_keys or [])
        self.calls: list[tuple[list[str], dict | None]] = []
        self._next_id = 1000

    def __call__(self, args, payload=None):
        self.calls.append((list(args), payload))
        path = args[-1]
        method = args[args.index("-X") + 1] if "-X" in args else "GET"
        if path.endswith("/keys"):
            return list(self.keys)
        if path.endswith("/rulesets") and method == "GET":
            return [{"id": r["id"], "name": r["name"], "enforcement": r["enforcement"]} for r in self.live]
        if path.endswith("/rulesets") and method == "POST":
            self._next_id += 1
            stored = self._store(payload, self._next_id)
            self.live.append(stored)
            return stored
        if "/rulesets/" in path and method == "GET":
            rid = int(path.rsplit("/", 1)[1])
            return next(r for r in self.live if r["id"] == rid)
        if "/rulesets/" in path and method == "PUT":
            rid = int(path.rsplit("/", 1)[1])
            stored = self._store(payload, rid)
            self.live = [stored if r["id"] == rid else r for r in self.live]
            return stored
        raise AssertionError(f"unstubbed gh api call: {args}")

    @staticmethod
    def _store(payload, rid):
        """GitHub's observed normalization, so the tests exercise the same
        readback shape production does: falsy rule parameters are dropped,
        and a DeployKey bypass actor's `actor_id` becomes null (measured --
        see rulesets.py's module docstring)."""
        rules = []
        for rule in payload["rules"]:
            params = {k: v for k, v in (rule.get("parameters") or {}).items() if v}
            rules.append({"type": rule["type"], **({"parameters": params} if params else {})})
        actors = []
        for a in payload.get("bypass_actors") or []:
            a = dict(a)
            if a.get("actor_type") == "DeployKey":
                a["actor_id"] = None
            actors.append(a)
        return {**payload, "id": rid, "rules": rules, "bypass_actors": actors, "enforcement": payload["enforcement"]}


class RulesetsTestCase(HermeticCase):
    def install(self, stub):
        real = rulesets.gh_api
        real_ver = rulesets._gh_version
        rulesets.gh_api = stub
        rulesets._gh_version = lambda: "stub"
        self.addCleanup(lambda: setattr(rulesets, "gh_api", real))
        self.addCleanup(lambda: setattr(rulesets, "_gh_version", real_ver))
        return stub

    def apply(self, argv, stub):
        self.install(stub)
        return self.run_main(["apply", *argv])

    def run_main(self, argv):
        """`main` with its console output captured -- the gate's fleet-tests
        stage log should carry test results, not three instrument headers per
        test. The captured text lands in `self.out`; assertions below pass it
        as their failure message so a red test still shows what apply said."""
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            rc = rulesets.main(["--repo", REPO, *argv])
        self.out = buf.getvalue()
        return rc


# --------------------------------------------------------------------- #
# The declaration itself
# --------------------------------------------------------------------- #


class TestDeclaration(HermeticCase):
    def test_exactly_the_five_named_rulesets(self):
        self.assertEqual(
            [r["name"] for r in rulesets.RULESETS],
            ["tip-guard", "rescued-guard", "proof-guard", "tip-update", "proof-update"],
        )

    def test_main_is_not_declared_here(self):
        """`main` has its own pre-existing ruleset with required_signatures
        and a pull_request rule. Declaring it here would let an `apply` PUT
        those away."""
        self.assertNotIn("main", rulesets.DECLARED_NAMES)

    def test_guards_have_no_bypass_actors(self):
        """Property 1. A bypass actor bypasses the whole ruleset, so a guard
        with any bypass stops being a guard for that actor."""
        for spec in rulesets.RULESETS:
            if spec["kind"] == "guard":
                self.assertEqual(spec["bypass_actors"], [], spec["name"])

    def test_guards_block_deletion_and_non_fast_forward(self):
        for spec in rulesets.RULESETS:
            if spec["kind"] == "guard":
                types = {r["type"] for r in spec["rules"]}
                self.assertEqual(types, {"deletion", "non_fast_forward"}, spec["name"])

    def test_guards_do_not_block_creation(self):
        """`train.py` L624-627 creates a new `rescued/<slug>` on every land,
        and the live proof test creates `keel-proof/x` on its first run. A
        `creation` rule here would break both."""
        for spec in rulesets.RULESETS:
            if spec["kind"] == "guard":
                self.assertNotIn("creation", {r["type"] for r in spec["rules"]})

    def test_tip_and_proof_guards_are_the_same_rules(self):
        """Property 2: the live test exercises `keel-proof/*` and claims the
        result transfers to the tip. It transfers only while these match."""
        by_name = {r["name"]: r for r in rulesets.RULESETS}
        self.assertEqual(
            rulesets._norm_rules(by_name["tip-guard"]["rules"]),
            rulesets._norm_rules(by_name["proof-guard"]["rules"]),
        )
        self.assertEqual(
            rulesets._norm_rules(by_name["tip-update"]["rules"]),
            rulesets._norm_rules(by_name["proof-update"]["rules"]),
        )

    def test_update_rulesets_carry_the_unresolved_sentinel(self):
        for spec in rulesets.RULESETS:
            if spec["kind"] == "update":
                self.assertEqual(spec["bypass_actors"], [rulesets.BYPASS_KEEL_TRAIN], spec["name"])

    def test_the_two_protected_ref_patterns_are_the_spec_ones(self):
        by_name = {r["name"]: r for r in rulesets.RULESETS}
        self.assertEqual(
            by_name["tip-guard"]["conditions"]["ref_name"]["include"],
            ["refs/heads/refactor/tag-machinery"],
        )
        self.assertEqual(
            by_name["rescued-guard"]["conditions"]["ref_name"]["include"],
            ["refs/heads/rescued/*"],
        )
        self.assertEqual(
            by_name["proof-guard"]["conditions"]["ref_name"]["include"],
            ["refs/heads/keel-proof/*"],
        )

    def test_tip_ref_matches_train_py(self):
        """One string, two files. `train.py` pushes to TIP_REF; the ruleset
        that protects it must name the same ref."""
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
        import train  # noqa: E402

        self.assertEqual(rulesets.TIP_REF, train.TIP_REF)


# --------------------------------------------------------------------- #
# The `--skip-update-rulesets` default
# --------------------------------------------------------------------- #


class TestSkipUpdateDefault(HermeticCase):
    def test_default_is_on(self):
        args = rulesets.build_parser().parse_args(["apply"])
        self.assertTrue(args.skip_update_rulesets)

    def test_explicit_flag_keeps_it_on(self):
        args = rulesets.build_parser().parse_args(["apply", "--skip-update-rulesets"])
        self.assertTrue(args.skip_update_rulesets)

    def test_negated_flag_turns_it_off(self):
        args = rulesets.build_parser().parse_args(["apply", "--no-skip-update-rulesets"])
        self.assertFalse(args.skip_update_rulesets)

    def test_selected_omits_update_rulesets_by_default(self):
        self.assertEqual(
            [r["name"] for r in rulesets.selected(skip_update=True)], list(rulesets.GUARD_NAMES)
        )
        self.assertEqual(
            [r["name"] for r in rulesets.selected(skip_update=False)], list(rulesets.DECLARED_NAMES)
        )


# --------------------------------------------------------------------- #
# Bypass resolution -- property 3
# --------------------------------------------------------------------- #


class TestBypassResolution(RulesetsTestCase):
    def test_refuses_when_the_deploy_key_does_not_exist(self):
        self.install(StubGh(deploy_keys=[]))
        with self.assertRaises(rulesets.RulesetError) as cm:
            rulesets._resolve_bypass_actors([rulesets.BYPASS_KEEL_TRAIN], REPO)
        self.assertIn("keel-train", str(cm.exception))

    def test_refuses_when_a_differently_titled_key_exists(self):
        self.install(StubGh(deploy_keys=[{"id": 7, "title": "someone-elses", "read_only": False}]))
        with self.assertRaises(rulesets.RulesetError):
            rulesets._resolve_bypass_actors([rulesets.BYPASS_KEEL_TRAIN], REPO)

    def test_refuses_when_a_second_write_key_exists(self):
        """A DeployKey bypass actor is not a particular key -- GitHub nulls
        the id and the bypass covers every write key on the repo (measured;
        see rulesets.py). So a second write key silently widens tip-push
        authority, and this is the only place that can notice."""
        self.install(StubGh(deploy_keys=[
            {"id": 7, "title": "keel-train", "read_only": False},
            {"id": 8, "title": "ci", "read_only": False},
        ]))
        with self.assertRaises(rulesets.RulesetError) as cm:
            rulesets._resolve_bypass_actors([rulesets.BYPASS_KEEL_TRAIN], REPO)
        self.assertIn("ALL", str(cm.exception))

    def test_a_second_READ_ONLY_key_is_tolerated(self):
        self.install(StubGh(deploy_keys=[
            {"id": 7, "title": "keel-train", "read_only": False},
            {"id": 8, "title": "mirror", "read_only": True},
        ]))
        actors = rulesets._resolve_bypass_actors([rulesets.BYPASS_KEEL_TRAIN], REPO)
        self.assertEqual(
            actors, [{"actor_id": 7, "actor_type": "DeployKey", "bypass_mode": "always"}]
        )

    def test_apply_refuses_update_rulesets_and_exits_nonzero(self):
        stub = StubGh(deploy_keys=[])
        rc = self.apply(["--no-skip-update-rulesets", "--dry-run"], stub)
        self.assertEqual(rc, 1, self.out)
        self.assertIn("tip-update: REFUSED", self.out)
        self.assertIn("proof-update: REFUSED", self.out)
        # ...and did not create anything.
        self.assertEqual([c for c in stub.calls if "-X" in c[0]], [])

    def test_readback_warn_fires_when_github_nulls_the_actor_id(self):
        warns = rulesets._readback_warns(
            [{"actor_id": 7, "actor_type": "DeployKey", "bypass_mode": "always"}],
            [{"actor_id": None, "actor_type": "DeployKey", "bypass_mode": "always"}],
        )
        self.assertTrue(any("EVERY write deploy key" in w for w in warns))


# --------------------------------------------------------------------- #
# apply: idempotence, blast radius
# --------------------------------------------------------------------- #


def _methods(stub):
    return [c[0][c[0].index("-X") + 1] for c in stub.calls if "-X" in c[0]]


class TestApply(RulesetsTestCase):
    def test_creates_the_three_guards_on_an_empty_repo(self):
        stub = StubGh(live_rulesets=[])
        self.assertEqual(self.apply([], stub), 0, self.out)
        self.assertEqual(_methods(stub), ["POST", "POST", "POST"], self.out)
        self.assertEqual(
            sorted(r["name"] for r in stub.live), sorted(rulesets.GUARD_NAMES)
        )

    def test_second_run_writes_nothing(self):
        stub = StubGh(live_rulesets=[])
        self.apply([], stub)
        stub.calls.clear()
        self.assertEqual(self.run_main(["apply"]), 0)
        self.assertEqual(_methods(stub), [], f"idempotent apply issued a write\n{self.out}")

    def test_drift_is_repaired_with_PUT_not_POST(self):
        stub = StubGh(live_rulesets=[])
        self.apply([], stub)
        # Someone widens tip-guard by hand: enforcement dropped to disabled.
        for r in stub.live:
            if r["name"] == "tip-guard":
                r["enforcement"] = "disabled"
        stub.calls.clear()
        self.assertEqual(self.run_main(["apply"]), 0)
        self.assertEqual(_methods(stub), ["PUT"], self.out)
        self.assertEqual(
            next(r["enforcement"] for r in stub.live if r["name"] == "tip-guard"), "active"
        )

    def test_dry_run_writes_nothing(self):
        stub = StubGh(live_rulesets=[])
        self.assertEqual(self.apply(["--dry-run"], stub), 0)
        self.assertEqual(_methods(stub), [], self.out)

    def test_never_deletes_and_never_touches_an_undeclared_ruleset(self):
        """Blast radius: `main` must come out the far side byte-identical."""
        main = {
            "id": 20593899, "name": "main", "enforcement": "active", "target": "branch",
            "conditions": {"ref_name": {"include": ["refs/heads/main"], "exclude": []}},
            "rules": [{"type": "deletion"}, {"type": "non_fast_forward"},
                      {"type": "required_signatures"}, {"type": "pull_request"}],
            "bypass_actors": [],
        }
        stub = StubGh(live_rulesets=[dict(main)])
        self.assertEqual(self.apply([], stub), 0, self.out)
        self.assertNotIn("DELETE", _methods(stub))
        for args, _payload in stub.calls:
            self.assertNotIn("repos/swack-tools/oxidex/rulesets/20593899", args)
        self.assertEqual(next(r for r in stub.live if r["name"] == "main"), main)


# --------------------------------------------------------------------- #
# comparable(): the normalization idempotence depends on
# --------------------------------------------------------------------- #


class TestComparable(HermeticCase):
    def test_false_rule_parameters_compare_equal_to_absent_ones(self):
        """GitHub stores `{"type":"update","parameters":{
        "update_allows_fetch_and_merge":false}}` as bare `{"type":"update"}`
        (measured). Without this, every apply would see drift and PUT."""
        sent = {"target": "branch", "enforcement": "active", "conditions": {},
                "rules": [{"type": "update", "parameters": {"update_allows_fetch_and_merge": False}}],
                "bypass_actors": []}
        got = {"target": "branch", "enforcement": "active", "conditions": {},
               "rules": [{"type": "update"}], "bypass_actors": []}
        self.assertEqual(rulesets.comparable(sent), rulesets.comparable(got))

    def test_rule_order_is_not_semantic(self):
        a = {"rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}
        b = {"rules": [{"type": "non_fast_forward"}, {"type": "deletion"}]}
        self.assertEqual(rulesets.comparable(a), rulesets.comparable(b))

    def test_a_missing_rule_is_drift(self):
        a = {"rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}
        b = {"rules": [{"type": "deletion"}]}
        self.assertNotEqual(rulesets.comparable(a), rulesets.comparable(b))

    def test_an_added_bypass_actor_is_drift(self):
        a = {"rules": [], "bypass_actors": []}
        b = {"rules": [], "bypass_actors": [
            {"actor_id": None, "actor_type": "DeployKey", "bypass_mode": "always"}]}
        self.assertNotEqual(rulesets.comparable(a), rulesets.comparable(b))


if __name__ == "__main__":
    unittest.main(verbosity=2)
