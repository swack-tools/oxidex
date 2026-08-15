"""train.py tests -- fixture hub, mock gate (injected), no Rust anywhere.

The gate-invocation-count assertions are the arithmetic the design sells:
N clean branches -> 1 gate; N with one poison -> bounded by bisect.

`TestUnionRegate`, `TestSingleton` and `TestCasRetire` below pin the three
defects an adversarial review confirmed on 2026-08-15, before this module
had ever run in production. Each of those tests fails on the pre-fix train:

  * the union re-gate skip pushed a tree whose exact member set had just
    gated FAIL (the `len(survivors) != len(members)` guard, train.py:291);
  * the staging-ref retirement was a raw `git push --delete`, which
    discards a commit pushed during the 20-45 minute gate window;
  * the claim key was the caller's own epoch, so two trains claimed two
    different refs and never contended at all.

Every gate here is a Python function; nothing in this file builds Rust,
runs gate.sh, or touches a hub outside the per-test tempdir.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import train
from claim import claim_ref
from fleetlib import Hub

TIP = "refs/heads/refactor/tag-machinery"
ENV = None


def gated_set(label: str) -> frozenset:
    """The member set a gate label names. Labels are `slug+slug+...`, with
    `+retry` appended on the post-ABORT retry -- parsed, never substring
    matched, so a slug that is a prefix of another cannot be confused for
    it."""
    return frozenset(p for p in label.split("+") if p and p != "retry")


def sh(args, cwd=None):
    global ENV
    if ENV is None:
        ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
               "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
    return subprocess.run(["git"] + args, cwd=cwd, check=True, env=ENV,
                          capture_output=True, text=True)


class TrainBase(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self.tmpdir.name)
        assert str(self.tmp).startswith(tempfile.gettempdir())
        self.bare = self.tmp / "hub.git"
        sh(["init", "-q", "--bare", str(self.bare)])
        w = self.tmp / "seed"
        sh(["init", "-q", str(w)])
        (w / "base.txt").write_text("base\n")
        (w / "fleet").mkdir()
        (w / "fleet" / "domains.toml").write_text('domains = [\n  "census.rs",\n]\n')
        sh(["add", "."], cwd=w)
        sh(["commit", "-qm", "tip"], cwd=w)
        sh(["push", "-q", str(self.bare), f"HEAD:{TIP}"], cwd=w)
        self.seed = w
        self.hub = Hub(str(self.bare), workdir=self.tmp / "cache")
        # Never the real ~/git/oxidex.git/train.token: an unrelated file on
        # the developer's box must not decide whether these tests exercise
        # the token path.
        self._token_env = os.environ.get(train.TRAIN_TOKEN_ENV)
        os.environ[train.TRAIN_TOKEN_ENV] = str(self.tmp / "no-such-train.token")

    def tearDown(self):
        if self._token_env is None:
            os.environ.pop(train.TRAIN_TOKEN_ENV, None)
        else:
            os.environ[train.TRAIN_TOKEN_ENV] = self._token_env
        self.tmpdir.cleanup()

    # -- fixture helpers ------------------------------------------------ #

    def tip_sha(self):
        return self.hub.sha(TIP)

    def staging_refs(self):
        return self.hub.list("refs/heads/staging/")

    def enable_push_options(self):
        """Record every push's ref + push options in the bare hub, the way
        the R1 `update` hook will read them. Without
        `receive.advertisePushOptions` a client `--push-option` is a hard
        `fatal:` -- which is itself worth pinning."""
        log = self.bare / "pushopts.log"
        sh(["config", "receive.advertisePushOptions", "true"], cwd=self.bare)
        hook = self.bare / "hooks" / "pre-receive"
        hook.write_text(
            "#!/bin/sh\n"
            "opts=''\n"
            "i=0\n"
            'while [ "$i" -lt "${GIT_PUSH_OPTION_COUNT:-0}" ]; do\n'
            '  eval "v=\\$GIT_PUSH_OPTION_$i"\n'
            '  opts="$opts $v"\n'
            "  i=$((i+1))\n"
            "done\n"
            "while read -r old new ref; do\n"
            f'  echo "$ref |$opts" >> {log}\n'
            "done\n"
            "exit 0\n"
        )
        hook.chmod(0o755)
        return log

    def push_option_log(self, log: Path) -> dict:
        """{refname: [push options]} from the fixture's pre-receive log."""
        out = {}
        if not log.is_file():
            return out
        for line in log.read_text().splitlines():
            ref, _, opts = line.partition("|")
            out[ref.strip()] = opts.split()
        return out

    def add_branch(self, slug, files: dict):
        w = self.seed
        sh(["checkout", "-q", "-B", f"b-{slug}",
            sh(["ls-remote", str(self.bare), TIP], cwd=w).stdout.split()[0]], cwd=w)
        for name, content in files.items():
            p = w / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content)
        sh(["add", "."], cwd=w)
        sh(["commit", "-qm", f"work {slug}"], cwd=w)
        sh(["push", "-q", str(self.bare), f"HEAD:refs/heads/staging/{slug}"], cwd=w)

    def run_train(self, gate_results: dict, batch_max=8, gate_fn=None, epoch="t"):
        """gate_results: {label-substring: verdict}; default PASS. Counts
        every invocation. `gate_fn` overrides it entirely for tests that
        need set-precise poisoning."""
        calls = []

        def gate(clone, label):
            calls.append(label)
            for frag, v in gate_results.items():
                if frag in label:
                    return v
            return "PASS"

        chosen = gate
        if gate_fn is not None:
            def chosen(clone, label, _inner=gate_fn):  # noqa: F811
                calls.append(label)
                return _inner(clone, label)

        res = train.run_train(str(self.bare), self.tmp, gate_fn=chosen, epoch=epoch,
                              batch_max=batch_max,
                              hub_workdir=self.tmp / "traincache")
        return res, calls


class TestBatching(TrainBase):
    def test_clean_batch_is_one_gate_and_refs_retired(self):
        for i in range(4):
            self.add_branch(f"c{i}", {f"file{i}.txt": f"c{i}\n"})
        res, calls = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        self.assertEqual(res.gate_invocations, 1, "4 disjoint branches must cost ONE gate")
        self.assertEqual(len(res.landed), 4)
        # staging refs retired, rescued refs verified in place
        left = self.hub.list("refs/heads/staging/")
        self.assertEqual(len(left), 0)
        rescued = self.hub.list("refs/heads/rescued/")
        self.assertEqual(len(rescued), 4)
        # tip advanced
        self.assertIsNotNone(res.new_tip)

    def test_poisoned_batch_bisects_and_survivors_land(self):
        for i in range(4):
            self.add_branch(f"p{i}", {f"pf{i}.txt": f"p{i}\n"})
        # any gate whose label includes p2 alone or in a group... we poison
        # by exact member: FAIL whenever p2 is part of the gated set.
        res, calls = self.run_train({"p2": "FAIL"})
        self.assertEqual(res.outcome, "advanced")
        self.assertEqual(len(res.landed), 3)
        self.assertIn(("staging/p2", "gate FAIL"), [(b, r) for b, r in res.ejected])
        self.assertLessEqual(res.gate_invocations, 2 * 2 + 1 + 1,
                             f"bisect must be bounded; used {res.gate_invocations}: {calls}")

    def test_conflict_domain_rides_solo(self):
        self.add_branch("normal", {"n.txt": "n\n"})
        self.add_branch("censusy", {"census.rs": "invariant\n"})
        res, calls = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        # two gate invocations: one solo, one batch -- never combined
        self.assertEqual(res.gate_invocations, 2)
        for label in calls:
            self.assertFalse("censusy" in label and "normal" in label,
                             f"domain branch was batched with another: {label}")

    def test_overlapping_write_sets_not_cobatched(self):
        self.add_branch("w1", {"same.txt": "one\n"})
        self.add_branch("w2", {"same.txt": "two\n"})
        res, calls = self.run_train({})
        # first takes same.txt; second must be excluded from this run
        self.assertEqual(len(res.landed), 1)
        left = self.hub.list("refs/heads/staging/")
        self.assertEqual(len(left), 1, "excluded branch stays queued for the next run")

    def test_abort_retries_once_and_does_not_condemn(self):
        self.add_branch("a1", {"a.txt": "a\n"})
        seen = {"n": 0}

        def gate(clone, label):
            seen["n"] += 1
            return "ABORT" if seen["n"] == 1 else "PASS"

        res = train.run_train(str(self.bare), self.tmp, gate_fn=gate, epoch="t",
                              hub_workdir=self.tmp / "traincache")
        self.assertEqual(res.outcome, "advanced")
        self.assertEqual(res.gate_invocations, 2)
        self.assertEqual(len(res.landed), 1)

    def test_merge_conflict_ejects_cleanly(self):
        # branch conflicting with a tip that has since changed same file
        self.add_branch("ok", {"okf.txt": "ok\n"})
        self.add_branch("confl", {"base.txt": "branch version\n"})
        # advance tip changing base.txt so confl conflicts
        w = self.seed
        sh(["checkout", "-q", "-B", "tipmove",
            sh(["ls-remote", str(self.bare), TIP], cwd=w).stdout.split()[0]], cwd=w)
        (w / "base.txt").write_text("tip moved\n")
        sh(["add", "."], cwd=w)
        sh(["commit", "-qm", "tip change"], cwd=w)
        sh(["push", "-q", str(self.bare), f"HEAD:{TIP}"], cwd=w)

        res, calls = self.run_train({})
        self.assertIn(("staging/confl", "merge-conflict"), res.ejected)
        self.assertEqual(len(res.landed), 1)

    def test_dry_run_touches_nothing(self):
        self.add_branch("d1", {"d.txt": "d\n"})
        before_tip = self.hub.sha(TIP)
        import contextlib, io
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            train.run_train(str(self.bare), self.tmp, gate_fn=lambda c, l: "PASS",
                            epoch="t", dry_run=True, hub_workdir=self.tmp / "traincache")
        self.assertIn("BATCH", buf.getvalue())
        self.assertEqual(self.hub.sha(TIP), before_tip)
        self.assertEqual(len(self.hub.list("refs/heads/staging/")), 1)

    def test_dry_run_does_not_take_the_singleton_claim(self):
        # A dry run writes nothing; if it took the claim it could block the
        # real train for a full lease.
        self.add_branch("d2", {"d2.txt": "d\n"})
        seen = []

        def gate(clone, label):
            seen.append(label)
            return "PASS"

        import contextlib, io
        with contextlib.redirect_stdout(io.StringIO()):
            train.run_train(str(self.bare), self.tmp, gate_fn=gate, epoch="t",
                            dry_run=True, hub_workdir=self.tmp / "traincache")
        self.assertEqual(seen, [])
        self.assertEqual(self.hub.list("refs/fleet/claims/"), {})


# ---------------------------------------------------------------------- #
# BUG 1 -- the reassembled union must be gated as the set it actually is
# ---------------------------------------------------------------------- #


class TestUnionRegate(TrainBase):
    """A survivor union is pushable only if a gate PASSed that EXACT set.

    The pre-fix guard was `if survivors and len(survivors) != len(members)`,
    which skipped the re-gate whenever every member survived its half --
    precisely the shape of an interaction failure -- and returned the full
    membership, so `run_train` pushed the tree whose gate had just said
    FAIL.
    """

    def poison(self, *bad_sets):
        """A gate that FAILs iff the gated set is a superset of any of
        `bad_sets`. Set-precise: a slug is never matched as a substring."""
        bad = [frozenset(b) for b in bad_sets]

        def gate(clone, label):
            got = gated_set(label)
            return "FAIL" if any(b <= got for b in bad) else "PASS"

        return gate

    def test_pair_fails_while_each_member_passes_alone_lands_nothing(self):
        for slug in ("u0", "u1"):
            self.add_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        before = self.tip_sha()
        res, calls = self.run_train({}, gate_fn=self.poison({"u0", "u1"}))

        self.assertEqual(res.landed, [], "a tree that gated FAIL must never land")
        self.assertEqual(self.tip_sha(), before, "tip must not move")
        self.assertEqual(res.outcome, "all-ejected")
        # Both branches stay on the hub, queued for a future composition.
        self.assertEqual(sorted(self.staging_refs()),
                         ["refs/heads/staging/u0", "refs/heads/staging/u1"])
        self.assertEqual(self.hub.list("refs/heads/rescued/"), {})
        for branch, why in res.ejected:
            self.assertIn("union gate FAIL", why)
        self.assertEqual({b for b, _ in res.ejected},
                         {"staging/u0", "staging/u1"})
        # The union WAS gated -- as the union, not merely as its halves.
        self.assertIn(frozenset({"u0", "u1"}), [gated_set(c) for c in calls])
        self.assertEqual(res.gate_invocations, 3, f"union + 2 singles: {calls}")

    def test_three_members_all_surviving_halves_land_nothing(self):
        # The literal shape the length heuristic mishandled: the whole
        # FAILs, every half and every single PASSes, so survivors ==
        # members and the old code skipped the re-gate entirely.
        for slug in ("ra", "rb", "rc"):
            self.add_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        before = self.tip_sha()
        res, calls = self.run_train({}, gate_fn=self.poison({"ra", "rb", "rc"}))

        self.assertEqual(res.landed, [])
        self.assertEqual(self.tip_sha(), before)
        self.assertEqual(len(self.staging_refs()), 3, "nothing retired")
        self.assertEqual([gated_set(c) for c in calls],
                         [frozenset({"ra", "rb", "rc"}),
                          frozenset({"ra"}),
                          frozenset({"rb", "rc"})])
        self.assertEqual(res.gate_invocations, 3)
        # Evidence emitted, not swallowed: a file-disjoint set that fails
        # only together is an undeclared conflict domain.
        self.assertTrue(res.missing_domain_proposals)

    def test_reassembled_union_is_gated_and_lands_on_pass(self):
        # Single culprit rc: survivors {ra, rb} are a set no gate has seen,
        # so the reassembly MUST be gated -- and only then may it land.
        for slug in ("ra", "rb", "rc"):
            self.add_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        res, calls = self.run_train({}, gate_fn=self.poison({"rc"}))

        sets = [gated_set(c) for c in calls]
        self.assertEqual(sets,
                         [frozenset({"ra", "rb", "rc"}),   # whole: FAIL
                          frozenset({"ra"}),               # half: PASS
                          frozenset({"rb", "rc"}),         # half: FAIL
                          frozenset({"rb"}),               # PASS
                          frozenset({"rc"}),               # FAIL -> ejected
                          frozenset({"ra", "rb"})])        # REASSEMBLY
        self.assertEqual(res.gate_invocations, 6)
        self.assertEqual(sorted(res.landed), ["staging/ra", "staging/rb"])
        self.assertIn(("staging/rc", "gate FAIL"), res.ejected)
        self.assertEqual(sorted(self.staging_refs()), ["refs/heads/staging/rc"])
        self.assertEqual(sorted(self.hub.list("refs/heads/rescued/")),
                         ["refs/heads/rescued/ra", "refs/heads/rescued/rb"])

    def test_reassembled_union_that_fails_lands_nothing(self):
        # Same bisect path, but the reassembled {ra, rb} is itself poison:
        # "only lands if that gate PASSes" is the half of the rule that a
        # green-only test cannot see.
        for slug in ("ra", "rb", "rc"):
            self.add_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        before = self.tip_sha()
        res, calls = self.run_train({}, gate_fn=self.poison({"rc"}, {"ra", "rb"}))

        sets = [gated_set(c) for c in calls]
        self.assertIn(frozenset({"ra", "rb"}), sets, "reassembly must be gated")
        self.assertEqual(res.landed, [])
        self.assertEqual(self.tip_sha(), before)
        self.assertEqual(len(self.staging_refs()), 3)
        self.assertEqual(self.hub.list("refs/heads/rescued/"), {})
        self.assertEqual(res.gate_invocations, 6,
                         f"and it must TERMINATE, not re-gate forever: {calls}")
        reasons = dict(res.ejected)
        self.assertEqual(reasons["staging/rc"], "gate FAIL")
        self.assertIn("union gate FAIL", reasons["staging/ra"])
        self.assertIn("union gate FAIL", reasons["staging/rb"])

    def test_member_ejected_during_reassembly_is_not_reported_as_landed(self):
        # `_rebuild`'s return value is what is in the tree; the final
        # reassembly used to discard it and hand back the requested list,
        # so a member ejected there would be rescued and retired while
        # absent from the tip. assemble_batch's disjointness makes a real
        # re-merge ejection hard to reach from the outside, which is
        # exactly why it needs pinning here rather than being left to
        # whether a conflict happens to be constructible.
        for slug in ("ra", "rb", "rc"):
            self.add_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        real_merge = train.merge_members
        fired = []

        def flaky_merge(clone, tip_sha, members):
            names = {c.slug for c in members}
            if names == {"ra", "rb"} and not fired:
                fired.append(True)
                keep = [c for c in members if c.slug != "rb"]
                merged, ejected = real_merge(clone, tip_sha, keep)
                dropped = [c for c in members if c.slug == "rb"]
                return merged, ejected + [(dropped[0], "merge-conflict")]
            return real_merge(clone, tip_sha, members)

        with mock.patch.object(train, "merge_members", flaky_merge):
            res, _calls = self.run_train({}, gate_fn=self.poison({"rc"}))

        self.assertTrue(fired, "the reassembly never happened")
        self.assertEqual(res.landed, ["staging/ra"],
                         "only what is actually in the tree may be reported landed")
        self.assertEqual(sorted(self.hub.list("refs/heads/rescued/")),
                         ["refs/heads/rescued/ra"])
        self.assertEqual(sorted(self.staging_refs()),
                         ["refs/heads/staging/rb", "refs/heads/staging/rc"],
                         "an ejected member keeps its branch")

    def test_push_is_refused_if_the_survivor_set_was_never_gated(self):
        # The invariant restated at the push site. Forge a bisect that
        # returns members with an empty PASS memo -- the shape of the bug
        # -- and the train must refuse rather than advance the tip.
        self.add_branch("g1", {"g1.txt": "g\n"})
        before = self.tip_sha()
        real = train._gate_and_bisect

        def unproven(clone, tip_sha, members, gate_fn, res, memo=None):
            real(clone, tip_sha, members, gate_fn, res, memo)
            if memo is not None:
                memo.passed.clear()
            return members

        with mock.patch.object(train, "_gate_and_bisect", unproven):
            with self.assertRaises(train.TrainError) as ctx:
                self.run_train({})
        self.assertIn("never gated as exactly that set", str(ctx.exception))
        self.assertEqual(self.tip_sha(), before)


# ---------------------------------------------------------------------- #
# BUG 2 -- retirement is a compare-and-swap, not a delete
# ---------------------------------------------------------------------- #


class TestCasRetire(TrainBase):
    def test_branch_moved_during_the_gate_is_not_deleted(self):
        self.add_branch("mv", {"mv.txt": "mv\n"})
        gated_sha = self.staging_refs()["refs/heads/staging/mv"]

        def gate(clone, label):
            # The author pushes to their branch while the gate runs -- 20
            # to 45 minutes of window, in production.
            w = self.seed
            sh(["checkout", "-q", "b-mv"], cwd=w)
            (w / "mv2.txt").write_text("more work\n")
            sh(["add", "."], cwd=w)
            sh(["commit", "-qm", "work after the train read the ref"], cwd=w)
            sh(["push", "-q", "-f", str(self.bare), "HEAD:refs/heads/staging/mv"], cwd=w)
            return "PASS"

        res, _calls = self.run_train({}, gate_fn=gate)
        moved_sha = self.staging_refs().get("refs/heads/staging/mv")

        self.assertEqual(res.outcome, "advanced")
        self.assertEqual(res.landed, ["staging/mv"], "the gated commit did land")
        # The new commit survives: the delete was refused, loudly.
        self.assertIsNotNone(moved_sha, "a moved staging ref must NOT be deleted")
        self.assertNotEqual(moved_sha, gated_sha)
        self.assertEqual(len(res.retire_failures), 1)
        branch, why = res.retire_failures[0]
        self.assertEqual(branch, "staging/mv")
        self.assertIn("NOT retired", why)
        self.assertIn(gated_sha[:12], why)
        # Rescue still pins exactly what was gated.
        self.assertEqual(self.hub.list("refs/heads/rescued/"),
                         {"refs/heads/rescued/mv": gated_sha})

    def test_unmoved_branch_is_retired_and_nothing_is_reported(self):
        self.add_branch("st", {"st.txt": "st\n"})
        res, _ = self.run_train({})
        self.assertEqual(res.retire_failures, [])
        self.assertEqual(self.staging_refs(), {})

    def test_no_raw_delete_push_remains_in_train(self):
        # R9's repo-wide grep in one file, at the source of the finding.
        src = (Path(train.__file__)).read_text()
        self.assertNotIn('"--delete"', src)
        self.assertNotIn("'--delete'", src)


# ---------------------------------------------------------------------- #
# BUG 3 -- the singleton claim actually contends
# ---------------------------------------------------------------------- #


class TestSingleton(TrainBase):
    def test_two_concurrent_runs_and_only_one_advances(self):
        self.add_branch("cc", {"cc.txt": "cc\n"})
        before = self.tip_sha()
        at_gate = threading.Event()
        finish = threading.Event()
        out = {}

        def gate_a(clone, label):
            at_gate.set()
            finish.wait(60)
            return "PASS"

        def run_a():
            try:
                out["a"] = train.run_train(
                    str(self.bare), self.tmp, gate_fn=gate_a, epoch="epoch-A",
                    hub_workdir=self.tmp / "cache-a")
            except BaseException as exc:  # surfaced by the assertions below
                out["a-exc"] = exc

        t = threading.Thread(target=run_a, name="train-a")
        t.start()
        try:
            self.assertTrue(at_gate.wait(60), "first train never reached its gate")
            # The claim is on the CONSTANT key, with the epoch in the
            # payload. Keyed by epoch (the pre-fix shape) these two runs
            # would hold two different refs and both proceed.
            self.assertIsNotNone(self.hub.sha(claim_ref("train", "singleton")))
            self.assertIsNone(self.hub.sha(claim_ref("train", "epoch-A")))
            self.assertEqual(
                self.hub.read(claim_ref("train", "singleton"))["work_key"], "epoch-A")

            calls_b = []

            def gate_b(clone, label):
                calls_b.append(label)
                return "PASS"

            res_b = train.run_train(str(self.bare), self.tmp, gate_fn=gate_b,
                                    epoch="epoch-B", hub_workdir=self.tmp / "cache-b")
        finally:
            finish.set()
            t.join(120)

        self.assertNotIn("a-exc", out, f"first train raised: {out.get('a-exc')!r}")
        res_a = out["a"]
        self.assertEqual(res_b.outcome, "claim-held")
        self.assertEqual(calls_b, [], "the losing train must not gate anything")
        self.assertEqual(res_a.outcome, "advanced")
        self.assertEqual(res_a.landed, ["staging/cc"])
        self.assertNotEqual(self.tip_sha(), before)
        self.assertEqual(self.staging_refs(), {}, "exactly one run retired the ref")
        # Released on the way out -- the next run is not blocked.
        self.assertIsNone(self.hub.sha(claim_ref("train", "singleton")))

    def test_claim_is_released_after_a_normal_run(self):
        self.add_branch("rel", {"rel.txt": "r\n"})
        self.run_train({})
        self.assertEqual(self.hub.list("refs/fleet/claims/"), {})


# ---------------------------------------------------------------------- #
# R1 -- the tip push carries the train token
# ---------------------------------------------------------------------- #


class TestTipToken(TrainBase):
    def test_token_file_absent_pushes_without_the_option(self):
        # Pre-rollout: the hook does not exist yet and the train must work.
        log = self.enable_push_options()
        self.add_branch("t0", {"t0.txt": "t\n"})
        res, _ = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        self.assertEqual(self.push_option_log(log)[TIP], [])

    def test_token_file_present_is_sent_on_the_tip_push_only(self):
        log = self.enable_push_options()
        tok = self.tmp / "train.token"
        tok.write_text("s3cr3t-value\n")
        tok.chmod(0o600)
        os.environ[train.TRAIN_TOKEN_ENV] = str(tok)

        self.add_branch("t1", {"t1.txt": "t\n"})
        res, _ = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        seen = self.push_option_log(log)
        self.assertEqual(seen[TIP], ["train-token=s3cr3t-value"])
        # The secret goes to the protected ref and nowhere else.
        self.assertEqual(seen["refs/heads/rescued/t1"], [])

    def test_options_helper_reads_env_then_default(self):
        tok = self.tmp / "tok"
        os.environ[train.TRAIN_TOKEN_ENV] = str(tok)
        self.assertEqual(train.tip_push_options(), [])          # absent
        tok.write_text("  abc \n")
        self.assertEqual(train.tip_push_options(), ["--push-option=train-token=abc"])
        tok.write_text("\n")
        self.assertEqual(train.tip_push_options(), [])          # empty != ""
        os.environ.pop(train.TRAIN_TOKEN_ENV)
        self.assertEqual(train.train_token_path(),
                         Path("~/git/oxidex.git/train.token").expanduser())

    def test_hub_without_advertised_push_options_still_advances(self):
        # A hub that cannot receive push options cannot be enforcing them
        # either; a half-finished rollout must not wedge the train.
        tok = self.tmp / "train.token"
        tok.write_text("s3cr3t\n")
        os.environ[train.TRAIN_TOKEN_ENV] = str(tok)
        self.add_branch("t2", {"t2.txt": "t\n"})
        res, _ = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        self.assertEqual(res.landed, ["staging/t2"])


class TestRealGateRefLifecycle(TrainBase):
    """`real_gate` is the production adapter; only its ref handling is
    exercised here -- gate.sh itself is never run, and nothing builds."""

    def _clone(self) -> Path:
        c = self.tmp / "gclone"
        sh(["clone", "-q", str(self.bare), str(c)])
        sh(["checkout", "-q", "-B", "m", self.tip_sha()], cwd=c)
        return c

    def _no_gate_script(self):
        real_run = subprocess.run

        def dispatch(cmd, *a, **kw):
            if cmd and cmd[0] == "bash":
                return subprocess.CompletedProcess(cmd, 0, "", "")
            return real_run(cmd, *a, **kw)

        return mock.patch.object(train.subprocess, "run", dispatch)

    def test_temp_ref_is_pushed_then_cas_deleted(self):
        clone = self._clone()
        hub = Hub(str(self.bare), workdir=self.tmp / "rgcache")
        with self._no_gate_script():
            verdict = train.real_gate(clone, "x", hub=hub)
        # No verdict file was written, so the adapter reports ABORT rather
        # than inventing a PASS -- and its temp ref is gone either way.
        self.assertEqual(verdict, "ABORT")
        self.assertEqual(
            [r for r in self.hub.list("refs/heads/staging/") if "train-tmp" in r], [])

    def test_temp_ref_moved_by_someone_else_is_not_deleted(self):
        clone = self._clone()
        hub = Hub(str(self.bare), workdir=self.tmp / "rgcache")
        real_delete = hub.delete
        moved = {}

        def steal_then_delete(ref, expect_sha):
            # Somebody force-pushes over our temp ref between the gate
            # finishing and the cleanup.
            w = self.seed
            sh(["checkout", "-q", "-B", "steal", self.tip_sha()], cwd=w)
            (w / "stolen.txt").write_text("not ours\n")
            sh(["add", "."], cwd=w)
            sh(["commit", "-qm", "steal"], cwd=w)
            sh(["push", "-q", "-f", str(self.bare), f"HEAD:{ref}"], cwd=w)
            moved[ref] = sh(["rev-parse", "HEAD"], cwd=w).stdout.strip()
            return real_delete(ref, expect_sha=expect_sha)

        with self._no_gate_script(), mock.patch.object(hub, "delete", steal_then_delete):
            train.real_gate(clone, "y", hub=hub)
        self.assertEqual(len(moved), 1)
        ref, sha = next(iter(moved.items()))
        self.assertEqual(self.hub.sha(ref), sha, "a ref we no longer own must survive")


if __name__ == "__main__":
    unittest.main()
