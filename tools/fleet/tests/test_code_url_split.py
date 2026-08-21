#!/usr/bin/env python3
"""Tests for the `code_url` split (docs/AGENT-SERVER-SPEC.md §4.4, Stage 1
task 3 of docs/AGENT-SERVER-PLAN.md): once the fleet spine is two GitHub
repos instead of one -- a PUBLIC code repo carrying `refs/heads/*` (the tip,
`staging/*`) and a PRIVATE state repo carrying `refs/fleet/*` -- three
call sites that used to fetch branch/tree history from `hub.url` must fetch
it from `hub.code_url` instead:

  * `workqueue.Queue._fetch_for_ancestry`  (workqueue.py)
  * `dispatch._have_objects` / `dispatch.merge_tree_sha`  (dispatch.py)
  * `train._run_train_locked`'s initial clone  (train.py)

`code_url` defaults to `url` on `fleetlib.Hub`, so every one of these sites
falls back to `getattr(hub, "code_url", hub.url)` -- this file's own `Hub`
fixtures never need the real attribute to exist, matching the plan's
"coordinate with nothing" note: whichever lands first (this file's
`getattr` fallback, or a real `Hub.code_url` from the parallel fleetlib
task) the other still passes.

Two real bare repos stand in for the split, never one repo wearing two
hats: `state.git` gets nothing but `refs/fleet/*` CAS payloads, exactly the
way `fleetlib.Hub.create`/`read` write them in production; `code.git` gets
a real tip commit plus two staging branches with real commit history,
because `git merge-base`/`git merge-tree` need real objects, not orphan
payload commits. `state.git` deliberately NEVER receives the code refs, so
a call site that regresses to `hub.url` fails LOUD -- an actual git error
from fetching a ref that is not there -- rather than quietly succeeding
against the wrong repo by coincidence.

Instrument: plain `unittest`, standard library only, against throwaway
`git init --bare` repos under `tempfile.gettempdir()` -- never the
production hub or code repo.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import dispatch  # noqa: E402
import train  # noqa: E402
import workqueue  # noqa: E402
from fleetlib import Hub, HubUnreachableError  # noqa: E402

TIP_REF = "refs/heads/refactor/tag-machinery"
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}


def _run(args, cwd=None, check=True):
    import os
    return subprocess.run(
        args, cwd=cwd, check=check, capture_output=True, text=True,
        env={**os.environ, **GIT_ENV},
    )


class _RepoPair:
    """`state.git` (empty of code refs -- the STATE hub after the split)
    and `code.git` (the tip plus two staging branches with real commit
    history -- the CODE hub after the split), plus a seed worktree used
    only to build that history."""

    def __init__(self, tmp: Path):
        assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
        self.tmp = tmp
        self.state = tmp / "state.git"
        self.code = tmp / "code.git"
        self.workdir = tmp / "cache"
        self.work = tmp / "seed"
        _run(["git", "init", "-q", "--bare", str(self.state)])
        _run(["git", "init", "-q", "--bare", str(self.code)])
        _run(["git", "init", "-q", str(self.work)])
        self.tip_sha = self._seed_code()

    def _git(self, *args, check=True):
        return _run(["git", "-C", str(self.work), *args], check=check)

    def _seed_code(self) -> str:
        (self.work / "domains.toml").write_text("[[domain]]\nname = \"root\"\n")
        self._git("add", ".")
        self._git("commit", "-qm", "root")
        self._git("branch", "-M", "refactor/tag-machinery")
        tip = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "refactor/tag-machinery")

        # staging/alpha: real drift off the tip, never merged into it --
        # `_is_ancestor` must say False, and its own commit's object must
        # only be reachable through `code.git`.
        self._git("checkout", "-q", "-b", "staging/alpha")
        (self.work / "alpha.txt").write_text("alpha\n")
        self._git("add", "alpha.txt")
        self._git("commit", "-qm", "alpha work")
        self.alpha_sha = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "staging/alpha:refs/heads/staging/alpha")

        # staging/merged: already an ancestor of the tip (== the tip
        # itself) -- `_is_ancestor` must say True.
        self._git("checkout", "-q", "refactor/tag-machinery")
        self._git("push", "-q", str(self.code), "refactor/tag-machinery:refs/heads/staging/merged")
        self.merged_sha = tip
        return tip

    def hub(self, *, with_code_url: bool, workdir_name: str = "cache") -> Hub:
        """A `Hub` whose `.url` is the STATE repo. `code_url` is set
        directly on the instance when requested -- the same shape a real
        `code_url`-aware `Hub.__init__` would leave it in, and exactly what
        every production call site's `getattr(hub, "code_url", hub.url)`
        fallback is there to tolerate either way."""
        h = Hub(str(self.state), workdir=self.tmp / workdir_name)
        if with_code_url:
            h.code_url = str(self.code)
        return h

    def combined_hub(self, workdir_name: str = "cache-combined") -> Hub:
        """Pre-split shape: `.url` IS the code repo, no `code_url`
        attribute at all -- "defaults to `url`, so nothing changes when
        unset"."""
        return Hub(str(self.code), workdir=self.tmp / workdir_name)

    def staging_dict(self) -> dict:
        return {
            "alpha": ("refs/heads/staging/alpha", self.alpha_sha),
            "merged": ("refs/heads/staging/merged", self.merged_sha),
        }


class _FixtureCase(unittest.TestCase):
    def setUp(self):
        self._tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmpdir.name)
        self.repos = _RepoPair(self.tmp)

    def tearDown(self):
        self._tmpdir.cleanup()


# --------------------------------------------------------------------- #
# workqueue.Queue._fetch_for_ancestry
# --------------------------------------------------------------------- #


class TestWorkqueueFetchForAncestry(_FixtureCase):
    def test_reads_branch_history_from_code_url_not_state_url(self):
        hub = self.repos.hub(with_code_url=True)
        queue = workqueue.Queue(hub, tip_ref=TIP_REF)
        staging = self.repos.staging_dict()
        cache_ns = queue._fetch_for_ancestry(self.repos.tip_sha, staging)
        try:
            self.assertFalse(
                queue._is_ancestor(cache_ns, self.repos.alpha_sha, self.repos.tip_sha),
                msg="staging/alpha drifted off the tip and must not read as an ancestor",
            )
            self.assertTrue(
                queue._is_ancestor(cache_ns, self.repos.merged_sha, self.repos.tip_sha),
                msg="staging/merged IS the tip and must read as an ancestor",
            )
        finally:
            queue._cleanup_cache(cache_ns, staging)

    def test_raises_when_code_url_absent_and_state_url_lacks_the_history(self):
        # No code_url set -> falls back to hub.url == state.git, which has
        # none of these branches. Proves the fallback is real (it truly
        # tries hub.url), not a silent no-op that happens to pass anyway.
        hub = self.repos.hub(with_code_url=False)
        queue = workqueue.Queue(hub, tip_ref=TIP_REF)
        with self.assertRaises(HubUnreachableError):
            queue._fetch_for_ancestry(self.repos.tip_sha, self.repos.staging_dict())

    def test_still_works_when_code_and_state_are_one_combined_hub(self):
        # "code_url defaults to url, so nothing changes when unset."
        hub = self.repos.combined_hub()
        queue = workqueue.Queue(hub, tip_ref=TIP_REF)
        staging = self.repos.staging_dict()
        cache_ns = queue._fetch_for_ancestry(self.repos.tip_sha, staging)
        try:
            self.assertTrue(queue._is_ancestor(cache_ns, self.repos.merged_sha, self.repos.tip_sha))
        finally:
            queue._cleanup_cache(cache_ns, staging)


# --------------------------------------------------------------------- #
# dispatch._have_objects / dispatch.merge_tree_sha
# --------------------------------------------------------------------- #


class TestDispatchHaveObjects(_FixtureCase):
    def test_reads_objects_and_computes_merge_tree_from_code_url(self):
        hub = self.repos.hub(with_code_url=True)
        self.assertTrue(
            dispatch._have_objects(hub, [TIP_REF, "refs/heads/staging/alpha"])
        )
        tree = dispatch.merge_tree_sha(hub, self.repos.tip_sha, self.repos.alpha_sha)
        self.assertIsNotNone(tree, "alpha only adds a file onto the tip -- the merge must succeed")

    def test_fails_closed_when_code_url_absent_and_state_url_lacks_the_objects(self):
        hub = self.repos.hub(with_code_url=False)
        self.assertFalse(
            dispatch._have_objects(hub, [TIP_REF, "refs/heads/staging/alpha"]),
            msg="hub.url (state.git) has neither ref; the fetch must fail, not silently succeed",
        )

    def test_still_works_when_code_and_state_are_one_combined_hub(self):
        hub = self.repos.combined_hub()
        self.assertTrue(
            dispatch._have_objects(hub, [TIP_REF, "refs/heads/staging/alpha"])
        )
        tree = dispatch.merge_tree_sha(hub, self.repos.tip_sha, self.repos.alpha_sha)
        self.assertIsNotNone(tree)


# --------------------------------------------------------------------- #
# train._run_train_locked's initial clone
# --------------------------------------------------------------------- #


class _StopAfterClone(Exception):
    """Raised by the `train._git` stub right after it records the `clone`
    command's source, so the test never has to stand up the rest of the
    train pipeline (gate, bisect, push) just to observe one `git clone`
    argument."""


class TestTrainClonesFromCodeUrl(_FixtureCase):
    def _capture_clone_src(self, hub: Hub, hub_url: str) -> list:
        seen: list = []
        real_git = train._git

        def fake_git(args, cwd=None, check=True):
            if args and args[0] == "clone":
                seen.append(args[2])
                raise _StopAfterClone()
            return real_git(args, cwd=cwd, check=check)

        with mock.patch.object(train, "_git", side_effect=fake_git):
            with self.assertRaises(_StopAfterClone):
                train._run_train_locked(
                    hub, hub_url,
                    gate_fn=lambda *_a, **_k: "PASS",
                    batch_max=10,
                    dry_run=True,
                    _clone_src=None,
                )
        return seen

    def test_clones_from_code_url_not_state_url(self):
        hub = self.repos.hub(with_code_url=True)
        seen = self._capture_clone_src(hub, str(self.repos.state))
        self.assertEqual(seen, [str(self.repos.code)])

    def test_clones_from_url_when_code_url_absent(self):
        hub = self.repos.hub(with_code_url=False)
        seen = self._capture_clone_src(hub, str(self.repos.state))
        self.assertEqual(seen, [str(self.repos.state)])

    def test_explicit_clone_src_still_wins_over_both(self):
        # `_clone_src` is the test-injection seam `run_train` itself
        # exposes; it must keep outranking code_url exactly as it always
        # outranked hub_url.
        hub = self.repos.hub(with_code_url=True)
        seen: list = []
        real_git = train._git

        def fake_git(args, cwd=None, check=True):
            if args and args[0] == "clone":
                seen.append(args[2])
                raise _StopAfterClone()
            return real_git(args, cwd=cwd, check=check)

        with mock.patch.object(train, "_git", side_effect=fake_git):
            with self.assertRaises(_StopAfterClone):
                train._run_train_locked(
                    hub, str(self.repos.state),
                    gate_fn=lambda *_a, **_k: "PASS",
                    batch_max=10,
                    dry_run=True,
                    _clone_src=str(self.repos.code) + "-explicit-override",
                )
        self.assertEqual(seen, [str(self.repos.code) + "-explicit-override"])


# --------------------------------------------------------------------- #
# refs/fleet/* stay on the state hub, unaffected by any of the above
# --------------------------------------------------------------------- #


class TestStateRefsStayOnStateUrl(_FixtureCase):
    def test_hub_cas_reads_and_writes_use_url_never_code_url(self):
        hub = self.repos.hub(with_code_url=True)
        ok = hub.create("refs/fleet/intents/demo", {"status": "open"})
        self.assertTrue(ok)

        payload = hub.read("refs/fleet/intents/demo")
        self.assertIsNotNone(payload)
        self.assertEqual(payload.get("status"), "open")

        # It must have landed on the STATE repo, never the code repo --
        # Hub.create/read never look at code_url.
        on_state = subprocess.run(
            ["git", "ls-remote", str(self.repos.state), "refs/fleet/intents/demo"],
            capture_output=True, text=True, check=True,
        )
        self.assertTrue(on_state.stdout.strip(), "payload must be on state.git")

        on_code = subprocess.run(
            ["git", "ls-remote", str(self.repos.code), "refs/fleet/intents/demo"],
            capture_output=True, text=True, check=True,
        )
        self.assertEqual(on_code.stdout.strip(), "", "payload must NOT be on code.git")


if __name__ == "__main__":
    unittest.main()
