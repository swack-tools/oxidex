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
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from _env import HermeticCase, scrub_env  # noqa: E402
from _fixtures import make_hub, within_sweep  # noqa: E402

import fleetlib
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
        ENV = scrub_env()  # pins the same t/t@t identity, drops FLEET_*/KEEL_*
    return subprocess.run(["git"] + args, cwd=cwd, check=True, env=ENV,
                          capture_output=True, text=True)


def _attempt_tip_bump(hub_url: str, sha_suffix: str, idx: int):
    """Top-level (picklable) worker for the multiprocessing tip-signal race
    test. Each worker is its own OS process racing the same bare hub's
    `refs/fleet/signals/tip` via `train.bump_tip_signal_via_hub` -- real
    inter-process contention on the actual CAS primitive, mirroring
    `test_drift_hook.TestConcurrentBumps` (the local-hook version of the
    same monotonic rule) and `test_fleetlib.TestConcurrentCreate`'s
    `_attempt_create` (one Hub, its own private workdir, per worker)."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    import train as _train  # re-import in the child process
    from fleetlib import Hub as _Hub

    workdir = tempfile.mkdtemp(prefix=f"tip-signal-race-{idx}-")
    try:
        hub = _Hub(url=hub_url, workdir=workdir)
        sha = (sha_suffix * 40)[:40]
        payload = _train.bump_tip_signal_via_hub(hub, sha)
        return payload["generation"], sha
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


class TrainBase(HermeticCase):
    def setUp(self):
        super().setUp()
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
        self.hub = make_hub(self, str(self.bare), workdir=self.tmp / "cache")
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
        # The train ran through its own plain Hub, out-of-band to the
        # fixture server; `list()` on claims is index-served by design
        # (cachedhub rule 3 covers sha/read, not listings), so poll the
        # listing empty across one sweep rather than asserting the index
        # was never behind.
        self.assertEqual(
            within_sweep(lambda: self.hub.list("refs/fleet/claims/"), lambda v: v == {}),
            {},
        )


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
        # `delete_code_ref`, not `delete`: the temp gate ref is
        # `staging/train-tmp-*`, a CODE ref, so its CAS cleanup now goes to
        # `code_push_url` (SPEC 4.4). The CAS itself is unchanged, which is
        # what this test pins.
        real_delete = hub.delete_code_ref
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

        with self._no_gate_script(), mock.patch.object(hub, "delete_code_ref", steal_then_delete):
            train.real_gate(clone, "y", hub=hub)
        self.assertEqual(len(moved), 1)
        ref, sha = next(iter(moved.items()))
        self.assertEqual(self.hub.sha(ref), sha, "a ref we no longer own must survive")


# ---------------------------------------------------------------------- #
# PLAN Stage 1 task 6 -- CAS-bump refs/fleet/signals/tip via Hub.update,
# replacing hooks/post-receive + drift.bump_tip_signal for the hubless
# interim (SPEC §3.1).
# ---------------------------------------------------------------------- #


class TestTipSignalBump(TrainBase):
    def test_direct_first_bump_creates_generation_one(self):
        sha = "a" * 40
        payload = train.bump_tip_signal_via_hub(self.hub, sha)
        self.assertEqual(payload["generation"], 1)
        self.assertEqual(payload["sha"], sha)
        self.assertEqual(payload["by"], "train")

        # `hub.read()` returns the fuller, `Hub._augment`-ed stored payload
        # (it adds `schema_version`/`written_by`/`written_at` provenance);
        # `bump_tip_signal_via_hub`'s return value is what it asked the hub
        # to write, so it is a subset, not the identical dict.
        stored = self.hub.read(train.TIP_SIGNAL_REF)
        for key, value in payload.items():
            self.assertEqual(stored[key], value)

    def test_direct_sequential_bumps_increment_generation(self):
        first = train.bump_tip_signal_via_hub(self.hub, "a" * 40)
        second = train.bump_tip_signal_via_hub(self.hub, "b" * 40)
        third = train.bump_tip_signal_via_hub(self.hub, "c" * 40)
        self.assertEqual(
            [first["generation"], second["generation"], third["generation"]], [1, 2, 3]
        )
        stored = self.hub.read(train.TIP_SIGNAL_REF)
        self.assertEqual(stored["generation"], 3)
        self.assertEqual(stored["sha"], "c" * 40)

    def test_custom_by_field_is_carried(self):
        payload = train.bump_tip_signal_via_hub(self.hub, "d" * 40, by="foreign")
        self.assertEqual(payload["by"], "foreign")

    def test_stale_expect_sha_is_refused_not_applied(self):
        """The CAS primitive the retry loop depends on: an update guarded
        by a sha that is no longer current is refused outright (`False`,
        never an exception, never a partial write) -- the hub-CAS analogue
        of `test_drift_hook.TestBumpTipSignalDirect.
        test_stale_cas_write_cannot_land`, which pins the same refusal one
        layer down (a raw `git update-ref <ref> <new> <stale-old>`)."""
        train.bump_tip_signal_via_hub(self.hub, "a" * 40)
        current_sha = self.hub.sha(train.TIP_SIGNAL_REF)

        stale_payload = {"sha": "b" * 40, "generation": 99, "ts": "x", "by": "train"}
        ok = self.hub.update(train.TIP_SIGNAL_REF, stale_payload, expect_sha="0" * 40)
        self.assertFalse(ok, "an update against a stale expect_sha must be refused, not applied")

        self.assertEqual(self.hub.sha(train.TIP_SIGNAL_REF), current_sha,
                          "a refused CAS write must leave the ref untouched")
        stored = self.hub.read(train.TIP_SIGNAL_REF)
        self.assertEqual(stored["generation"], 1, "the refused write must not have landed")

    def test_generation_is_monotonic_under_real_concurrent_bumps(self):
        """N real OS processes hammer `bump_tip_signal_via_hub` against the
        same bare hub concurrently. Requirement (same as
        `drift.bump_tip_signal`'s docstring): a losing racer re-reads the
        newer generation and retries, so nothing is ever lost or handed out
        twice -- verified by checking the set of generations handed out is
        exactly {1..N}: no duplicates and no gaps."""
        n = 8
        digits = "0123456789abcdef"
        with ProcessPoolExecutor(max_workers=n) as pool:
            futures = [pool.submit(_attempt_tip_bump, str(self.bare), digits[i], i)
                       for i in range(n)]
            results = [f.result() for f in as_completed(futures)]

        generations = sorted(gen for gen, _sha in results)
        self.assertEqual(
            generations, list(range(1, n + 1)),
            f"expected exactly the generations 1..{n} with no gaps or duplicates, got {generations}",
        )

        final = self.hub.read(train.TIP_SIGNAL_REF)
        self.assertEqual(final["generation"], n)
        winner_sha = next(sha for gen, sha in results if gen == n)
        self.assertEqual(final["sha"], winner_sha)

    def test_full_train_run_bumps_the_signal_to_the_new_tip(self):
        self.add_branch("sig", {"sig.txt": "s\n"})
        res, _ = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        # The train bumped the signal through its own plain Hub -- out of
        # band to the fixture server -- so poll the fixture hub's view
        # across one sweep instead of asserting the cache was never behind.
        signal = within_sweep(lambda: self.hub.read(train.TIP_SIGNAL_REF), lambda v: v is not None)
        self.assertIsNotNone(signal, "a landed train run must bump refs/fleet/signals/tip")
        self.assertEqual(signal["generation"], 1)
        self.assertEqual(signal["sha"], res.new_tip)
        self.assertEqual(signal["by"], "train")

    def test_two_sequential_train_runs_advance_the_generation(self):
        self.add_branch("sig1", {"a.txt": "a\n"})
        res1, _ = self.run_train({}, epoch="e1")
        # `add_branch` bases its new branch on the TIP sha it reads from
        # the hub, but checks it out in `self.seed` -- a clone the train
        # never pushes to, so it does not yet have the commit the first
        # run just advanced the tip to. Fetch it in before basing off it.
        sh(["fetch", "-q", str(self.bare), TIP], cwd=self.seed)
        self.add_branch("sig2", {"b.txt": "b\n"})
        res2, _ = self.run_train({}, epoch="e2")
        self.assertEqual([res1.outcome, res2.outcome], ["advanced", "advanced"])
        signal = self.hub.read(train.TIP_SIGNAL_REF)
        self.assertEqual(signal["generation"], 2)
        self.assertEqual(signal["sha"], res2.new_tip)

    def test_signal_bump_failure_does_not_fail_the_train_run(self):
        """Best-effort (SPEC 3.1: the signal is `fleetd`'s poll-latency
        fast path, never the source of truth -- TIP_REF itself is): a hub
        that can never win the CAS must not turn an already-landed tip
        push into a failed train run, the same shape as
        `_mark_intent_done`'s best-effort intent close."""
        self.add_branch("be", {"be.txt": "b\n"})
        with mock.patch.object(train, "bump_tip_signal_via_hub",
                                side_effect=train.HubError("boom")):
            res, _ = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        self.assertEqual(res.landed, ["staging/be"])
        self.assertIsNone(self.hub.read(train.TIP_SIGNAL_REF),
                           "no signal write landed, but the run must still succeed")


# ---------------------------------------------------------------------- #
# PLAN Stage 1 task 6 -- the tip push carries the train deploy key's
# GIT_SSH_COMMAND (SPEC §8) when FLEET_TRAIN_DEPLOY_KEY names a real file.
# ---------------------------------------------------------------------- #


class TestTrainDeployKeySsh(TrainBase):
    def setUp(self):
        super().setUp()
        # Never let the developer's own shell (or a prior test) leak a
        # deploy key path or an ambient GIT_SSH_COMMAND into this fixture.
        self._deploy_key_env = os.environ.get(train.TRAIN_DEPLOY_KEY_ENV)
        os.environ.pop(train.TRAIN_DEPLOY_KEY_ENV, None)
        self._ambient_ssh_cmd = os.environ.get("GIT_SSH_COMMAND")
        os.environ.pop("GIT_SSH_COMMAND", None)

    def tearDown(self):
        if self._deploy_key_env is None:
            os.environ.pop(train.TRAIN_DEPLOY_KEY_ENV, None)
        else:
            os.environ[train.TRAIN_DEPLOY_KEY_ENV] = self._deploy_key_env
        if self._ambient_ssh_cmd is None:
            os.environ.pop("GIT_SSH_COMMAND", None)
        else:
            os.environ["GIT_SSH_COMMAND"] = self._ambient_ssh_cmd
        super().tearDown()

    def test_no_key_configured_is_a_normal_state_not_an_error(self):
        self.assertIsNone(train._train_deploy_key_ssh_command())
        self.add_branch("nokey", {"n.txt": "n\n"})
        res, _ = self.run_train({})
        self.assertEqual(res.outcome, "advanced")
        self.assertNotIn("GIT_SSH_COMMAND", os.environ,
                          "no key configured must leave the ambient env untouched")

    def test_missing_key_file_warns_and_falls_back_to_none(self):
        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(self.tmp / "no-such-key")
        self.assertIsNone(train._train_deploy_key_ssh_command())

    def test_the_key_reaches_the_push_subprocess_and_never_os_environ(self):
        """NAME THE INSTRUMENT. The version of this test that shipped with
        the bug spied `Hub.push_ref` and read `os.environ` from inside it,
        so it asserted "the value was in the process environment while a
        push method ran" -- true of a mechanism that never reaches git at
        all, and true regardless of which repo the push targets or which
        ssh options survive. It was green while the feature was broken
        three ways at once.

        What is observed here instead is the `env=` dict actually handed to
        `subprocess.run`, which is the only thing git ever sees."""
        key = self.tmp / "deploy_key"
        key.write_text("fake key material\n")
        key.chmod(0o600)
        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(key)
        expected_ssh_command = train._train_deploy_key_ssh_command()
        self.assertIsNotNone(expected_ssh_command)
        self.assertIn(f"-i {key}", expected_ssh_command)
        for option in ("-o IdentitiesOnly=yes", "-o IdentityAgent=none",
                       "-o BatchMode=yes", "-o ConnectTimeout=10",
                       "-o StrictHostKeyChecking=accept-new"):
            self.assertIn(option, expected_ssh_command)

        calls = []
        real_run = fleetlib.subprocess.run

        def spy(cmd, **kw):
            calls.append({
                "argv": list(cmd),
                # Only commands `fleetlib` launched are in scope: it is
                # the only caller that hands git an explicit `env=`.
                # `train._git` (local merges, checkouts, the clone of the
                # public code repo) inherits os.environ, and `claim.py`'s
                # `rustc -vV` platform probe is given an env but is not a
                # git command and has no ssh transport to pin.
                "git": bool(cmd) and cmd[0] == "git" and kw.get("env") is not None,
                "env_ssh": (kw.get("env") or {}).get("GIT_SSH_COMMAND"),
                "os_environ_ssh": os.environ.get("GIT_SSH_COMMAND"),
            })
            return real_run(cmd, **kw)

        self.add_branch("key1", {"k.txt": "k\n"})
        with mock.patch.object(fleetlib.subprocess, "run", spy):
            res, _ = self.run_train({})

        self.assertEqual(res.outcome, "advanced")
        git_calls = [c for c in calls if c["git"]]
        self.assertTrue(git_calls)
        tip_pushes = [c for c in git_calls
                      if "push" in c["argv"]
                      and any(a.endswith(":" + TIP) for a in c["argv"])]
        self.assertEqual(len(tip_pushes), 1, msg=[c["argv"] for c in git_calls])
        self.assertEqual(tip_pushes[0]["env_ssh"], expected_ssh_command)

        # Every OTHER git command in the run -- claim create/renew, the
        # verdict reads, the tip-signal CAS, the rescue push, the staging
        # retirement -- ran under the pinned default.
        for call in git_calls:
            if call in tip_pushes:
                continue
            self.assertEqual(call["env_ssh"], fleetlib.DEFAULT_SSH_COMMAND,
                             msg=f"deploy key leaked onto {call['argv']}")

        # os.environ was never written, at any instant, by anyone.
        self.assertTrue(all(c["os_environ_ssh"] is None for c in calls))
        self.assertNotIn("GIT_SSH_COMMAND", os.environ)


if __name__ == "__main__":
    unittest.main()
