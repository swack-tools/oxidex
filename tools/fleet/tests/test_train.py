"""train.py tests -- fixture hub, mock gate (control-file driven), no Rust.

The gate-invocation-count assertions are the arithmetic the design sells:
N clean branches -> 1 gate; N with one poison -> bounded by bisect.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import train
from fleetlib import Hub

TIP = "refs/heads/refactor/tag-machinery"
ENV = None


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

    def tearDown(self):
        self.tmpdir.cleanup()

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

    def run_train(self, gate_results: dict, batch_max=8):
        """gate_results: {label-substring: verdict}; default PASS. Counts
        every invocation."""
        calls = []

        def gate(clone, label):
            calls.append(label)
            for frag, v in gate_results.items():
                if frag in label:
                    return v
            return "PASS"

        res = train.run_train(str(self.bare), self.tmp, gate_fn=gate, epoch="t",
                              batch_max=batch_max)
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

        res = train.run_train(str(self.bare), self.tmp, gate_fn=gate, epoch="t")
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
                            epoch="t", dry_run=True)
        self.assertIn("BATCH", buf.getvalue())
        self.assertEqual(self.hub.sha(TIP), before_tip)
        self.assertEqual(len(self.hub.list("refs/heads/staging/")), 1)


if __name__ == "__main__":
    unittest.main()
