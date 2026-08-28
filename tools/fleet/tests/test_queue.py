#!/usr/bin/env python3
"""Tests for tools/fleet/workqueue.py.

Everything here runs against a throwaway `git init --bare` repo created in
`setUp` -- never the production hub (work2.oxidex.net). Unlike
`test_fleetlib.py` / `test_claim.py`, these tests need *real* commit
history (an actual tip branch and actual staging branches with actual
parent/child relationships), not just orphan payload commits, because
`Queue` has to run `git merge-base --is-ancestor` against them. `setUp`
builds that history through a disposable working clone, then throws the
clone away -- the hub (bare repo) is the only thing that persists.

Plain `unittest`, standard library only (no pytest in this environment).

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from claim import Claim  # noqa: E402
from workqueue import INTENTS_PREFIX, Queue, QueueError  # noqa: E402
from _env import HermeticCase  # noqa: E402
from _fixtures import make_hub  # noqa: E402


def _run_git(args, cwd=None, input_bytes=None):
    result = subprocess.run(args, cwd=cwd, input=input_bytes, capture_output=True)
    if result.returncode != 0:
        raise RuntimeError(
            f"git {' '.join(args)} (cwd={cwd}) failed: {result.stderr.decode('utf-8', 'replace')}"
        )
    return result


class QueueTestCase(HermeticCase):
    """Base fixture: a throwaway bare repo with real commit history for the
    tip and a handful of staging branches, standing in for the hub.
    """

    TIP_REF = "refs/heads/refactor/tag-machinery"

    def setUp(self):
        super().setUp()
        self._tmp_root = tempfile.mkdtemp(prefix="queue-test-")
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        self.workdir = str(Path(self._tmp_root) / "cache")
        self._work_clone = str(Path(self._tmp_root) / "work")

        init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0)

        # Fixture guard: never let this point at the production hub.
        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

        self._init_history()

        self.hub = make_hub(self, self.hub_path, workdir=self.workdir)
        self.queue = Queue(self.hub, tip_ref=self.TIP_REF)

    def tearDown(self):
        shutil.rmtree(self._tmp_root, ignore_errors=True)

    # -- building real commit history ------------------------------------#

    def _git(self, *args):
        return _run_git(["git", *args], cwd=self._work_clone)

    def _init_history(self):
        _run_git(["git", "clone", "--quiet", self.hub_path, self._work_clone])
        env_cfg = [
            ["git", "config", "user.email", "queue-test@oxidex.local"],
            ["git", "config", "user.name", "queue-test"],
        ]
        for cmd in env_cfg:
            _run_git(cmd, cwd=self._work_clone)

        (Path(self._work_clone) / "README").write_text("root\n")
        self._git("add", "README")
        self._git("commit", "--quiet", "-m", "root commit")
        self._git("branch", "-M", "refactor/tag-machinery")
        self._git("push", "--quiet", "origin", "refactor/tag-machinery")

    def commit_on(self, branch, filename, content, base="refactor/tag-machinery"):
        """Create `branch` off `base` (creating it if needed), add one
        commit, and push it to the hub. Returns the new commit sha.
        """
        self._git("checkout", "--quiet", base)
        # Does the branch already exist locally?
        exists = subprocess.run(
            ["git", "rev-parse", "--verify", "--quiet", branch],
            cwd=self._work_clone,
            capture_output=True,
        )
        if exists.returncode == 0:
            self._git("checkout", "--quiet", branch)
        else:
            self._git("checkout", "--quiet", "-b", branch)

        (Path(self._work_clone) / filename).write_text(content)
        self._git("add", filename)
        self._git("commit", "--quiet", "-m", f"commit on {branch}")
        self._git("push", "--quiet", "origin", f"{branch}:{branch}")
        sha = self._git("rev-parse", "HEAD").stdout.decode().strip()
        self._git("checkout", "--quiet", "refactor/tag-machinery")
        return sha

    def advance_tip(self, filename, content):
        self._git("checkout", "--quiet", "refactor/tag-machinery")
        (Path(self._work_clone) / filename).write_text(content)
        self._git("add", filename)
        self._git("commit", "--quiet", "-m", "advance tip")
        self._git("push", "--quiet", "origin", "refactor/tag-machinery:refactor/tag-machinery")
        return self._git("rev-parse", "refactor/tag-machinery").stdout.decode().strip()

    def merge_into_tip(self, branch):
        """Fast-forward-merge `branch` into the tip, so it becomes an
        ancestor -- simulating a branch the train already landed.
        """
        self._git("checkout", "--quiet", "refactor/tag-machinery")
        self._git("merge", "--quiet", "--no-ff", "-m", f"merge {branch}", branch)
        self._git("push", "--quiet", "origin", "refactor/tag-machinery:refactor/tag-machinery")


# --------------------------------------------------------------------- #
# Fixture guard
# --------------------------------------------------------------------- #


class TestFixtureGuard(QueueTestCase):
    def test_hub_is_a_temp_path_never_the_production_hub(self):
        resolved = str(Path(self.hub.url).resolve())
        self.assertTrue(resolved.startswith(str(Path(tempfile.gettempdir()).resolve())))
        self.assertNotIn("work2.oxidex.net", resolved)


# --------------------------------------------------------------------- #
# Core queue = staging - ancestors - claims - withdrawn
# --------------------------------------------------------------------- #


class TestQueueComputation(QueueTestCase):
    def test_never_reads_a_local_queue_file(self):
        # Nothing in workqueue.py's public surface accepts a queue-file path,
        # and computing twice must not depend on any state left behind by
        # the first call.
        self.commit_on("staging/alpha", "alpha.txt", "alpha")
        first = self.queue.compute()
        second = self.queue.compute()
        self.assertEqual(set(first.keys()), set(second.keys()))
        self.assertIn("alpha", first)

    def test_plain_staging_branch_appears_in_queue(self):
        self.commit_on("staging/bravo", "bravo.txt", "bravo")
        result = self.queue.compute()
        self.assertIn("bravo", result)
        self.assertEqual(result["bravo"].ref, "refs/heads/staging/bravo")

    def test_branch_already_ancestor_of_tip_never_appears(self):
        self.commit_on("staging/charlie", "charlie.txt", "charlie")
        self.merge_into_tip("staging/charlie")

        result = self.queue.compute()
        self.assertNotIn("charlie", result, msg="an already-merged branch must not queue again")

    def test_branch_with_live_claim_never_appears(self):
        self.commit_on("staging/delta", "delta.txt", "delta")

        claim = Claim(self.hub, "gate", "some-tree-sha", work_key="delta", ttl=600)
        claim.acquire()
        try:
            result = self.queue.compute()
            self.assertNotIn("delta", result, msg="a branch with a live claim must not queue")
        finally:
            claim.release()

        # Once released, it's queueable again.
        result_after_release = self.queue.compute()
        self.assertIn("delta", result_after_release)

    def test_branch_with_expired_claim_still_appears(self):
        self.commit_on("staging/echo", "echo.txt", "echo")

        claim = Claim(self.hub, "gate", "some-tree-sha-2", work_key="echo", ttl=0)
        claim.acquire()  # expires immediately

        result = self.queue.compute()
        self.assertIn("echo", result, msg="an expired claim must not block the queue")

    def test_branch_with_withdrawn_intent_never_appears(self):
        self.commit_on("staging/foxtrot", "foxtrot.txt", "foxtrot")

        ok = self.hub.create(
            f"{INTENTS_PREFIX}/foxtrot",
            {
                "slug": "foxtrot",
                "title": "withdrawn intent",
                "scope": {"formats": [], "tags": [], "files": []},
                "status": "withdrawn",
                "claimed_by": "nobody",
                "created_at": "2026-01-01T00:00:00+00:00",
            },
        )
        self.assertTrue(ok)

        result = self.queue.compute()
        self.assertNotIn("foxtrot", result, msg="a branch whose intent is withdrawn must not queue")

    def test_branch_with_open_intent_still_appears(self):
        self.commit_on("staging/golf", "golf.txt", "golf")
        ok = self.hub.create(
            f"{INTENTS_PREFIX}/golf",
            {
                "slug": "golf",
                "title": "open intent",
                "scope": {"formats": [], "tags": [], "files": []},
                "status": "open",
                "claimed_by": "someone",
                "created_at": "2026-01-01T00:00:00+00:00",
            },
        )
        self.assertTrue(ok)

        result = self.queue.compute()
        self.assertIn("golf", result)

    def test_branch_retired_from_hub_never_appears(self):
        """A branch that was deleted from the hub entirely (retired) simply
        isn't in `staging/*` any more -- there is no local copy that could
        still remember it, so it structurally cannot appear.
        """
        self.commit_on("staging/hotel", "hotel.txt", "hotel")
        before = self.queue.compute()
        self.assertIn("hotel", before)

        # Retire it: delete the ref from the hub.
        sha = self.hub.sha("refs/heads/staging/hotel")
        self.assertIsNotNone(sha)
        _run_git(
            ["git", "push", self.hub_path, f":refs/heads/staging/hotel"],
            cwd=self._work_clone,
        )
        self.assertIsNone(self.hub.sha("refs/heads/staging/hotel"))

        after = self.queue.compute()
        self.assertNotIn("hotel", after)

    def test_multiple_branches_independent_filters_compose(self):
        self.commit_on("staging/india", "india.txt", "india")  # queued
        self.commit_on("staging/juliet", "juliet.txt", "juliet")
        self.merge_into_tip("staging/juliet")  # ancestor -- excluded
        self.commit_on("staging/kilo", "kilo.txt", "kilo")
        claim = Claim(self.hub, "gate", "kilo-tree", work_key="kilo", ttl=600)
        claim.acquire()  # claimed -- excluded
        self.addCleanup(claim.release)

        result = self.queue.compute()
        self.assertIn("india", result)
        self.assertNotIn("juliet", result)
        self.assertNotIn("kilo", result)

    def test_empty_staging_yields_empty_queue(self):
        self.assertEqual(self.queue.compute(), {})

    def test_missing_tip_raises_queue_error(self):
        bogus = Queue(self.hub, tip_ref="refs/heads/does-not-exist")
        with self.assertRaises(QueueError):
            bogus.compute()

    def test_slugs_returns_sorted_names(self):
        self.commit_on("staging/zulu", "zulu.txt", "zulu")
        self.commit_on("staging/alpha2", "alpha2.txt", "alpha2")
        self.assertEqual(self.queue.slugs(), sorted(self.queue.slugs()))
        self.assertIn("zulu", self.queue.slugs())
        self.assertIn("alpha2", self.queue.slugs())

    def test_compute_leaves_no_stray_local_refs_in_cache(self):
        """The disposable `refs/fleet-queue-cache/*` namespace used for the
        ancestry check must be cleaned up after each `compute()` call --
        it's a scratch cache, not a second source of truth.
        """
        self.commit_on("staging/lima", "lima.txt", "lima")
        self.queue.compute()

        list_refs = subprocess.run(
            ["git", "--git-dir", str(self.hub.workdir), "for-each-ref", "refs/fleet-queue-cache"],
            capture_output=True,
        )
        self.assertEqual(list_refs.stdout.strip(), b"", msg="queue cache refs were not cleaned up")


if __name__ == "__main__":
    unittest.main()
