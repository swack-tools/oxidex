#!/usr/bin/env python3
"""Tests for the code/state split (docs/AGENT-SERVER-SPEC.md §4.4's routing
table, Stage 1 of docs/AGENT-SERVER-PLAN.md): once the fleet spine is two
GitHub repos instead of one -- a PUBLIC code repo carrying `refs/heads/*`
(the tip, `staging/*`, `rescued/*`) and a PRIVATE state repo carrying
`refs/fleet/*` -- every call site that answers a *code* question has to be
routed at the code repo, and every code WRITE at the code push repo.

The table has three columns of consequence and this file pins all three:

  * code READS -> `hub.code_url`: `workqueue`'s tip sha, `staging/*`
    listing and ancestry fetch; `dispatch._have_objects` and its branch-sha
    probe; `train`'s clone and tip reads; `agentworker`'s clone and branch
    probes.
  * code WRITES -> `hub.code_push_url`: the train's tip advance,
    `rescued/*`, the `staging/*` retirement CAS, the temp gate ref.
  * coordination -> `hub.url`, unchanged, and asserted to have stayed
    there after a full train run.

Plus the two things that made the routing bugs survivable-looking:
`QueueError` from a missing tip must become a `refused` reason rather than
a daemon traceback, and the tip push's deploy key must reach ONE
subprocess's environment rather than `os.environ` (where the train
singleton's renewer thread inherits it).

Two real bare repos stand in for the split, never one repo wearing two
hats: `state.git` gets nothing but `refs/fleet/*` CAS payloads, exactly the
way `fleetlib.Hub.create`/`read` write them in production; `code.git` gets
a real tip commit plus two staging branches with real commit history,
because `git merge-base`/`git merge-tree` need real objects, not orphan
payload commits. `state.git` deliberately NEVER receives the code refs, so
a call site that regresses to `hub.url` fails LOUD -- an actual git error,
or a `None` this file asserts against -- rather than quietly succeeding
against the wrong repo by coincidence.

The tip in the fixture has ALSO moved past `staging/alpha`, which is not
decoration: with the tip as alpha's merge-base, `dispatch.economic_refusal`
correctly answers "no-drift" and the agentworker and train cases would exit
before reaching the call site under test.

Instrument: plain `unittest`, standard library only, against throwaway
`git init --bare` repos under `tempfile.gettempdir()` -- never the
production hub or code repo.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
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

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import agentworker  # noqa: E402
import dispatch  # noqa: E402
import fleetlib  # noqa: E402
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
        root = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "refactor/tag-machinery")

        # staging/merged: an ancestor of the tip -- `_is_ancestor` must say
        # True and the queue must filter it out as already landed.
        self._git("push", "-q", str(self.code),
                  "refactor/tag-machinery:refs/heads/staging/merged")
        self.merged_sha = root

        # staging/alpha: real drift off the tip, never merged into it --
        # `_is_ancestor` must say False, and its own commit's object must
        # only be reachable through `code.git`.
        self._git("checkout", "-q", "-b", "staging/alpha")
        (self.work / "alpha.txt").write_text("alpha\n")
        self._git("add", "alpha.txt")
        self._git("commit", "-qm", "alpha work")
        self.alpha_sha = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "staging/alpha:refs/heads/staging/alpha")

        # Then the tip moves ON ITS OWN. Without this, alpha's merge-base
        # IS the tip and `dispatch.economic_refusal` answers "no-drift" --
        # a correct refusal that would stop the agentworker and train cases
        # below before they reach the call site under test.
        self._git("checkout", "-q", "refactor/tag-machinery")
        (self.work / "tip2.txt").write_text("tip moved on\n")
        self._git("add", "tip2.txt")
        self._git("commit", "-qm", "tip advances past alpha")
        tip = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "refactor/tag-machinery")
        self.root_sha = root
        return tip

    def hub(self, *, with_code_url: bool, workdir_name: str = "cache") -> Hub:
        """A `Hub` whose `.url` is the STATE repo, built through the real
        constructor: `code_url`/`code_push_url` are `fleetlib.Hub`'s own
        arguments, so the fixture exercises the production shape rather
        than an attribute stapled on afterwards."""
        if with_code_url:
            return Hub(str(self.state), workdir=self.tmp / workdir_name,
                       code_url=str(self.code), code_push_url=str(self.code))
        return Hub(str(self.state), workdir=self.tmp / workdir_name)

    def refs_on(self, repo: Path, pattern: str) -> dict:
        """`{refname: sha}` read with a plain `git ls-remote` -- the
        instrument is git itself, never the code under test."""
        out = _run(["git", "ls-remote", str(repo), pattern]).stdout
        got = {}
        for line in out.splitlines():
            line = line.strip()
            if not line:
                continue
            sha, refname = line.split("\t", 1)
            got[refname] = sha
        return got

    def combined_hub(self, workdir_name: str = "cache-combined") -> Hub:
        """Pre-split shape: `.url` IS the code repo and `code_url`/
        `code_push_url` are left to default to it -- "defaults to `url`, so
        nothing changes when unset"."""
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


# --------------------------------------------------------------------- #
# B1 -- workqueue.compute() against a real split spine
# --------------------------------------------------------------------- #


class TestQueueComputeAgainstSplitSpine(_FixtureCase):
    """`Queue.compute()` asks two questions before it can answer anything:
    "where is the tip" and "what staging branches exist". Both were asked
    of `hub.url`. Against a state repo carrying only `refs/fleet/*` the
    first returns None -- so `compute()` raises `QueueError` on EVERY call
    -- and the second returns `{}`. `QueueError` is not a `HubError`, and
    `fleetd`'s reconcile loop catches exactly `HubError`, so the daemon
    died in a traceback before its first heartbeat.
    """

    def test_state_repo_really_has_no_code_refs(self):
        # The fixture's own premise, asserted rather than assumed: if
        # state.git ever grew a refs/heads/*, every test below would pass
        # for the wrong reason.
        self.assertEqual(self.repos.refs_on(self.repos.state, "refs/heads/*"), {})

    def test_compute_lists_staging_and_the_tip_from_the_code_repo(self):
        hub = self.repos.hub(with_code_url=True)
        queue = workqueue.Queue(hub, tip_ref=TIP_REF)
        got = queue.compute()
        self.assertEqual(sorted(got), ["alpha"])
        self.assertEqual(got["alpha"].sha, self.repos.alpha_sha)
        self.assertEqual(got["alpha"].ref, "refs/heads/staging/alpha")

    def test_compute_raises_queue_error_when_the_tip_is_looked_for_on_state(self):
        # The pre-fix routing, reproduced exactly: code_url == url == the
        # state repo. This is the traceback B1 names.
        hub = self.repos.hub(with_code_url=False)
        with self.assertRaises(workqueue.QueueError) as caught:
            workqueue.Queue(hub, tip_ref=TIP_REF).compute()
        self.assertIn(TIP_REF, str(caught.exception))
        self.assertNotIsInstance(caught.exception, fleetlib.HubError)

    def test_compute_or_refusal_returns_a_reason_instead_of_raising(self):
        hub = self.repos.hub(with_code_url=False)
        got, refusal = workqueue.Queue(hub, tip_ref=TIP_REF).compute_or_refusal()
        self.assertEqual(got, {})
        self.assertIsNotNone(refusal)
        reason, detail = refusal
        self.assertEqual(reason, "queue-unavailable")
        self.assertIn(TIP_REF, detail)

    def test_compute_or_refusal_is_transparent_on_the_happy_path(self):
        hub = self.repos.hub(with_code_url=True)
        queue = workqueue.Queue(hub, tip_ref=TIP_REF)
        got, refusal = queue.compute_or_refusal()
        self.assertIsNone(refusal)
        self.assertEqual(sorted(got), ["alpha"])

    def test_an_unreachable_code_repo_still_raises_rather_than_refusing(self):
        # A transport failure is not a configuration verdict: it must keep
        # its HubUnreachableError so fleetd's degrade-this-step path sees
        # it, instead of being flattened into a "refused" reason that reads
        # as permanent.
        hub = Hub(str(self.repos.state), workdir=self.tmp / "cache-gone",
                  code_url=str(self.tmp / "no-such-repo.git"))
        with self.assertRaises(HubUnreachableError):
            workqueue.Queue(hub, tip_ref=TIP_REF).compute_or_refusal()


# --------------------------------------------------------------------- #
# B2 -- the train writes CODE to the code repo and STATE to the state repo
# --------------------------------------------------------------------- #


class _TrainSplitBase(_FixtureCase):
    def setUp(self):
        super().setUp()
        # Never the developer's real ~/git/oxidex.git/train.token, and
        # never a deploy key leaked in from the ambient shell.
        self._saved_env = {
            k: os.environ.get(k)
            for k in (train.TRAIN_TOKEN_ENV, train.TRAIN_DEPLOY_KEY_ENV, "GIT_SSH_COMMAND")
        }
        os.environ[train.TRAIN_TOKEN_ENV] = str(self.tmp / "no-such-train.token")
        os.environ.pop(train.TRAIN_DEPLOY_KEY_ENV, None)
        os.environ.pop("GIT_SSH_COMMAND", None)

    def tearDown(self):
        for key, value in self._saved_env.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
        super().tearDown()

    def run_train(self, **kw):
        return train.run_train(
            str(self.repos.state),
            self.tmp,
            gate_fn=lambda clone, label: "PASS",
            epoch="split-test",
            hub_workdir=self.tmp / "traincache",
            code_url=str(self.repos.code),
            code_push_url=str(self.repos.code),
            **kw,
        )


class TestTrainLandsCodeOnCodeAndStateOnState(_TrainSplitBase):
    def test_one_run_puts_every_ref_on_the_repo_that_owns_it(self):
        res = self.run_train()
        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")
        self.assertEqual(res.landed, ["staging/alpha"])

        code_heads = self.repos.refs_on(self.repos.code, "refs/heads/*")
        new_tip = res.new_tip
        self.assertTrue(new_tip)

        # 1. the tip advanced ON THE CODE REPO
        self.assertEqual(code_heads.get(TIP_REF), new_tip)
        self.assertNotEqual(new_tip, self.repos.tip_sha)

        # 2. rescued/alpha landed on the code repo at the gated sha
        self.assertEqual(code_heads.get("refs/heads/rescued/alpha"),
                         self.repos.alpha_sha)

        # 3. staging/alpha was RETIRED from the code repo
        self.assertNotIn("refs/heads/staging/alpha", code_heads)

        # 4. the tip signal is a coordination ref: state repo, not code
        signal = self.repos.refs_on(self.repos.state, "refs/fleet/signals/*")
        self.assertIn("refs/fleet/signals/tip", signal)
        hub = self.repos.hub(with_code_url=True, workdir_name="verify")
        self.assertEqual((hub.read("refs/fleet/signals/tip") or {}).get("sha"), new_tip)

        # 5. NOTHING code-shaped leaked onto the state repo. This is the
        #    assertion the pre-fix train fails outright: its tip push, its
        #    staging retirement and its temp gate ref all aimed here.
        self.assertEqual(self.repos.refs_on(self.repos.state, "refs/heads/*"), {})

        # 6. ...and nothing coordination-shaped leaked onto the PUBLIC
        #    code repo. `refs/fleet/*` carries user@host:pid provenance
        #    (SPEC 8) and the code repo is public.
        self.assertEqual(self.repos.refs_on(self.repos.code, "refs/fleet/*"), {})

    def test_a_moved_staging_branch_is_not_retired(self):
        """The CAS survives the move to the code repo: `delete_code_ref`
        carries `--force-with-lease`, so an author who pushed during the
        gate window keeps their commit."""
        original = self.repos.alpha_sha
        moved = {}

        real_push_tip = train._push_tip

        def push_then_move(hub, clone, options):
            r = real_push_tip(hub, clone, options)
            # The author pushes during the (here, instantaneous) gate
            # window, after the train captured alpha's sha.
            _run(["git", "-C", str(self.repos.work), "checkout", "-q", "staging/alpha"])
            (self.repos.work / "late.txt").write_text("pushed during the gate\n")
            _run(["git", "-C", str(self.repos.work), "add", "late.txt"])
            _run(["git", "-C", str(self.repos.work), "commit", "-qm", "late work"])
            moved["sha"] = _run(
                ["git", "-C", str(self.repos.work), "rev-parse", "HEAD"]).stdout.strip()
            _run(["git", "-C", str(self.repos.work), "push", "-q", str(self.repos.code),
                  "staging/alpha:refs/heads/staging/alpha"])
            return r

        with mock.patch.object(train, "_push_tip", side_effect=push_then_move):
            res = self.run_train()

        self.assertEqual(res.outcome, "advanced")
        self.assertEqual([b for b, _why in res.retire_failures], ["staging/alpha"])
        code_heads = self.repos.refs_on(self.repos.code, "refs/heads/*")
        self.assertEqual(code_heads.get("refs/heads/staging/alpha"), moved["sha"])
        self.assertNotEqual(moved["sha"], original)


# --------------------------------------------------------------------- #
# B2/B3 -- the deploy key is a per-subprocess env, never os.environ
# --------------------------------------------------------------------- #


class _SubprocessSpy:
    """Records `(argv, env["GIT_SSH_COMMAND"])` for every git subprocess
    `fleetlib` launches, and the value of `os.environ["GIT_SSH_COMMAND"]`
    at that instant.

    The instrument matters here. The test this replaces spied
    `Hub.push_ref` and read `os.environ` from inside it, which passes
    whether or not the value ever reaches git -- it was green while the
    feature was broken in three separate ways (wrong repo, HTTPS URL a
    deploy key cannot authenticate, three pinned ssh options dropped).
    Reading the `env=` dict actually handed to `subprocess.run` is the
    only observation that distinguishes them.
    """

    def __init__(self):
        self.calls = []
        self._lock = threading.Lock()
        self._real = fleetlib.subprocess.run
        self.on_call = None

    def __call__(self, cmd, **kw):
        env = kw.get("env")
        record = {
            "argv": list(cmd),
            # `via_fleetlib`: `fleetlib.subprocess` and `train.subprocess`
            # are the same module object, so patching one patches both.
            # Only `fleetlib._raw_run` passes an explicit `env=`; the
            # train's own `_git` helper (local merges, checkouts, and the
            # clone of the PUBLIC code repo) inherits os.environ and is
            # not part of the routing contract under test here.
            "via_fleetlib": env is not None,
            "env_ssh": (env or {}).get("GIT_SSH_COMMAND"),
            "os_environ_ssh": os.environ.get("GIT_SSH_COMMAND"),
            "thread": threading.current_thread().name,
        }
        with self._lock:
            self.calls.append(record)
        if self.on_call is not None:
            self.on_call(record)
        return self._real(cmd, **kw)

    @property
    def fleetlib_calls(self) -> list:
        """The git commands `fleetlib` launched. Two exclusions, both
        deliberate: commands with no explicit `env=` are the train's own
        raw `_git` helper (local merges/checkouts and the clone of the
        PUBLIC code repo), and `claim.py`'s `rustc -vV` platform probe is
        given an env but is not a git command and has no ssh transport to
        pin."""
        return [c for c in self.calls
                if c["via_fleetlib"] and c["argv"] and c["argv"][0] == "git"]

    def pushes_to(self, url: str) -> list:
        return [c for c in self.fleetlib_calls
                if "push" in c["argv"] and url in c["argv"]]

    def touching(self, url: str) -> list:
        return [c for c in self.fleetlib_calls if url in c["argv"]]


class TestTrainDeployKeyIsScopedToOneSubprocess(_TrainSplitBase):
    def setUp(self):
        super().setUp()
        self.key = self.tmp / "train_deploy_key"
        self.key.write_text("not a real key, never offered to a real host\n")
        self.key.chmod(0o600)
        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(self.key)
        self.spy = _SubprocessSpy()

    def test_the_deploy_key_reaches_the_tip_push_and_nothing_else(self):
        expected = train._train_deploy_key_ssh_command()
        self.assertIsNotNone(expected)

        # B2's dropped options: the hand-rolled string had none of these.
        for option in ("-o BatchMode=yes", "-o ConnectTimeout=10",
                       "-o StrictHostKeyChecking=accept-new",
                       "-o IdentitiesOnly=yes", "-o IdentityAgent=none"):
            self.assertIn(option, expected)
        self.assertIn(f"-i {self.key}", expected)

        # While the deploy-key push is in flight, another thread does a
        # STATE write -- the shape of the train singleton's claim renewer,
        # which pushes to refs/fleet/claims/train/singleton every renew
        # interval for the whole 20-45 minute gate window. With the
        # override in os.environ it inherits the code repo's deploy key
        # and IdentitiesOnly=yes against a repo that has never seen it.
        concurrent = {}
        renewer_hub = Hub(str(self.repos.state), workdir=self.tmp / "renewer")

        def on_call(record):
            if record["env_ssh"] == expected and "renewer" not in record["thread"]:
                if "done" in concurrent:
                    return
                concurrent["done"] = True
                t = threading.Thread(
                    target=lambda: renewer_hub.create(
                        "refs/fleet/claims/train/singleton-probe", {"holder": "probe"}),
                    name="renewer-probe",
                )
                t.start()
                t.join(30)

        self.spy.on_call = on_call
        with mock.patch.object(fleetlib.subprocess, "run", self.spy):
            res = self.run_train()
        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")

        code = str(self.repos.code)
        state = str(self.repos.state)

        tip_pushes = [c for c in self.spy.pushes_to(code)
                      if any(a.endswith(":" + TIP_REF) for a in c["argv"])]
        self.assertTrue(self.spy.fleetlib_calls)
        self.assertEqual(len(tip_pushes), 1, msg=[c["argv"] for c in self.spy.calls])
        self.assertEqual(tip_pushes[0]["env_ssh"], expected)

        # os.environ was NEVER touched -- not during the push, not after.
        self.assertTrue(all(c["os_environ_ssh"] is None for c in self.spy.calls),
                        msg="the deploy key was exported process-wide")
        # ...which also means the train's own raw `_git` subprocesses,
        # which inherit os.environ wholesale, never saw it either.
        self.assertTrue(any(not c["via_fleetlib"] for c in self.spy.calls))
        self.assertNotIn("GIT_SSH_COMMAND", os.environ)

        # Every command aimed at the STATE repo -- claims, the attempt
        # ledger, the tip signal, the concurrent renewer probe -- ran under
        # the pinned default, never the deploy key.
        state_calls = self.spy.touching(state)
        self.assertTrue(state_calls)
        for call in state_calls:
            self.assertEqual(call["env_ssh"], fleetlib.DEFAULT_SSH_COMMAND,
                             msg=f"deploy key leaked onto {call['argv']}")

        # The concurrent thread really did run, and really did run clean.
        self.assertTrue(concurrent.get("done"), "the concurrency probe never fired")
        renewer_calls = [c for c in self.spy.fleetlib_calls
                         if c["thread"] == "renewer-probe"]
        self.assertTrue(renewer_calls, "the renewer thread issued no git command")
        for call in renewer_calls:
            self.assertEqual(call["env_ssh"], fleetlib.DEFAULT_SSH_COMMAND)

        # And the non-tip CODE writes (rescued/*, the staging retirement)
        # do not carry the key either: SPEC 3.1 routes them through the
        # PAT, and the ruleset bypass exists for the tip alone.
        other_code_pushes = [c for c in self.spy.pushes_to(code) if c not in tip_pushes]
        self.assertTrue(other_code_pushes)
        for call in other_code_pushes:
            self.assertEqual(call["env_ssh"], fleetlib.DEFAULT_SSH_COMMAND)

    def test_a_missing_key_file_is_normal_and_leaves_the_default(self):
        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(self.tmp / "absent")
        self.assertIsNone(train._train_deploy_key_ssh_command())
        with mock.patch.object(fleetlib.subprocess, "run", self.spy):
            res = self.run_train()
        self.assertEqual(res.outcome, "advanced")
        self.assertTrue(self.spy.fleetlib_calls)
        for call in self.spy.fleetlib_calls:
            self.assertEqual(call["env_ssh"], fleetlib.DEFAULT_SSH_COMMAND)


class TestAmbientSshCommandIsIgnored(_FixtureCase):
    """B3. `_raw_run` used to read `env.get("GIT_SSH_COMMAND", <default>)`,
    so any value inherited from the operator's shell, a systemd unit or a
    parent process replaced `BatchMode=yes`, `ConnectTimeout=10` and
    `StrictHostKeyChecking=accept-new` for every fleet git operation --
    silently, and with failure modes (a daemon blocked on a passphrase
    prompt; a two-minute connect stall inside a 30 s timeout) that name
    none of them."""

    def setUp(self):
        super().setUp()
        self._saved = os.environ.get("GIT_SSH_COMMAND")
        self.addCleanup(self._restore)
        self.spy = _SubprocessSpy()

    def _restore(self):
        if self._saved is None:
            os.environ.pop("GIT_SSH_COMMAND", None)
        else:
            os.environ["GIT_SSH_COMMAND"] = self._saved

    def test_an_ambient_value_never_reaches_git(self):
        os.environ["GIT_SSH_COMMAND"] = "ssh -o BatchMode=no -o ConnectTimeout=0"
        hub = self.repos.hub(with_code_url=True)
        with mock.patch.object(fleetlib.subprocess, "run", self.spy):
            hub.create("refs/fleet/intents/amb", {"status": "open"})
        self.assertTrue(self.spy.fleetlib_calls)
        for call in self.spy.fleetlib_calls:
            self.assertEqual(call["env_ssh"], fleetlib.DEFAULT_SSH_COMMAND)

    def test_an_explicit_parameter_is_the_only_way_to_change_it(self):
        hub = self.repos.hub(with_code_url=True)
        wanted = fleetlib.ssh_command(identity_file=str(self.tmp / "k"))
        with mock.patch.object(fleetlib.subprocess, "run", self.spy):
            hub.push_code_ref(f"{self.repos.tip_sha}:refs/heads/proof",
                              ssh_command=wanted)
        pushes = [c for c in self.spy.fleetlib_calls if "push" in c["argv"]]
        self.assertEqual(len(pushes), 1)
        self.assertEqual(pushes[0]["env_ssh"], wanted)
        self.assertNotIn("GIT_SSH_COMMAND", os.environ)

    def test_the_pinned_options_survive_an_identity_file(self):
        got = fleetlib.ssh_command(identity_file="/k/ey")
        for option in ("-o ConnectTimeout=10", "-o BatchMode=yes",
                       "-o StrictHostKeyChecking=accept-new",
                       "-o IdentitiesOnly=yes", "-o IdentityAgent=none"):
            self.assertIn(option, got)
        self.assertIn("-i /k/ey", got)
        self.assertEqual(fleetlib.ssh_command(), fleetlib.DEFAULT_SSH_COMMAND)


# --------------------------------------------------------------------- #
# B2 -- code pushes target code_push_url, which may differ from code_url
# --------------------------------------------------------------------- #


class TestCodePushUrl(_FixtureCase):
    def test_defaults_to_code_url_which_defaults_to_url(self):
        plain = Hub(str(self.repos.state), workdir=self.tmp / "c1")
        self.assertEqual(plain.code_push_url, plain.code_url)
        self.assertEqual(plain.code_url, plain.url)

        split = Hub(str(self.repos.state), workdir=self.tmp / "c2",
                    code_url=str(self.repos.code))
        self.assertEqual(split.code_push_url, str(self.repos.code))

    def test_reads_and_writes_can_use_different_code_remotes(self):
        """The real shape on a deploy-key host: reads over the cheap
        token-authenticated remote, writes over the one the key can
        authenticate. Two bare repos stand in for the two transports."""
        push_target = self.tmp / "code-push.git"
        _run(["git", "init", "-q", "--bare", str(push_target)])
        hub = Hub(str(self.repos.state), workdir=self.tmp / "c3",
                  code_url=str(self.repos.code), code_push_url=str(push_target))

        # read from code_url
        self.assertEqual(hub.code_sha(TIP_REF), self.repos.tip_sha)
        self.assertIn("refs/heads/staging/alpha", hub.code_list("refs/heads/staging"))

        # write to code_push_url
        _run(["git", "--git-dir", str(self.tmp / "c3"), "fetch", "--quiet",
              str(self.repos.code), TIP_REF])
        r = hub.push_code_ref(f"{self.repos.tip_sha}:refs/heads/proof")
        self.assertEqual(r.returncode, 0, msg=r.stderr)
        self.assertIn("refs/heads/proof",
                      self.repos.refs_on(push_target, "refs/heads/*"))
        self.assertNotIn("refs/heads/proof",
                         self.repos.refs_on(self.repos.code, "refs/heads/*"))
        self.assertEqual(self.repos.refs_on(self.repos.state, "refs/heads/*"), {})

    def test_delete_code_ref_is_a_cas_on_the_push_remote(self):
        hub = self.repos.hub(with_code_url=True)
        self.assertFalse(
            hub.delete_code_ref("refs/heads/staging/alpha", self.repos.tip_sha),
            msg="a stale expect_sha must lose the CAS, not delete the branch",
        )
        self.assertIn("refs/heads/staging/alpha",
                      self.repos.refs_on(self.repos.code, "refs/heads/*"))
        self.assertTrue(
            hub.delete_code_ref("refs/heads/staging/alpha", self.repos.alpha_sha))
        self.assertNotIn("refs/heads/staging/alpha",
                         self.repos.refs_on(self.repos.code, "refs/heads/*"))


# --------------------------------------------------------------------- #
# S2 -- agentworker clones CODE, never the state repo
# --------------------------------------------------------------------- #


class _StopAfterAgentClone(Exception):
    pass


class TestAgentworkerClonesCode(_FixtureCase):
    def setUp(self):
        super().setUp()
        self._saved_cli = os.environ.get("FLEET_AGENT_CLI_OVERRIDE")
        os.environ["FLEET_AGENT_CLI_OVERRIDE"] = "/usr/bin/true"
        self.addCleanup(self._restore_cli)
        # R7 (review finding): agentworker.run() hardcodes
        # `Path.home() / ".fleetd" / "agentcache"` as its Hub workdir, so
        # every test in this class was writing into the REAL
        # ~/.fleetd/agentcache on whatever machine ran the suite.
        # Redirect HOME into this fixture's own tempdir instead, same
        # convention test_bringup_split.py uses for its subprocess env.
        self._agent_home = self.tmp / "agent-home"
        self._agent_home.mkdir()
        self._home_patch = mock.patch.dict(os.environ, {"HOME": str(self._agent_home)})
        self._home_patch.start()
        self.addCleanup(self._home_patch.stop)

    def _restore_cli(self):
        if self._saved_cli is None:
            os.environ.pop("FLEET_AGENT_CLI_OVERRIDE", None)
        else:
            os.environ["FLEET_AGENT_CLI_OVERRIDE"] = self._saved_cli

    def _clone_src(self, **kw) -> list:
        seen: list = []
        real = agentworker.subprocess.run

        def fake_run(args, **kwargs):
            if len(args) > 2 and args[0] == "git" and args[1] == "clone":
                seen.append(args[3])
                raise _StopAfterAgentClone()
            return real(args, **kwargs)

        with mock.patch.object(agentworker.subprocess, "run", side_effect=fake_run):
            with self.assertRaises(_StopAfterAgentClone):
                agentworker.run("staging/alpha", str(self.repos.state), "t", **kw)
        return seen

    def test_clone_targets_the_code_repo_not_the_state_repo(self):
        self.assertEqual(self._clone_src(code_url=str(self.repos.code)),
                         [str(self.repos.code)])

    def test_code_url_defaults_to_the_hub_so_a_hub_only_spawn_still_works(self):
        """`fleetd.start_agent` passes `--hub` alone until its one-line
        `--code` addition lands. Defaulting keeps that spawn working
        against a single-repo topology instead of failing every agent."""
        seen: list = []
        real = agentworker.subprocess.run

        def fake_run(args, **kwargs):
            if len(args) > 2 and args[0] == "git" and args[1] == "clone":
                seen.append(args[3])
                raise _StopAfterAgentClone()
            return real(args, **kwargs)

        with mock.patch.object(agentworker.subprocess, "run", side_effect=fake_run):
            with self.assertRaises(_StopAfterAgentClone):
                agentworker.main(["--branch", "staging/alpha",
                                  "--hub", str(self.repos.code), "--host", "t"])
        self.assertEqual(seen, [str(self.repos.code)])

    def test_branch_and_tip_probes_read_the_code_repo(self):
        """Before the clone, `run()` probes the tip and the branch. Aimed
        at the state repo both answer None and the worker exits 5 --
        "missing tip or ref on hub" -- for a branch that plainly exists."""
        rc = agentworker.run("staging/alpha", str(self.repos.state), "t")
        self.assertEqual(rc, 5, "state-repo-only probes must not find code refs")


if __name__ == "__main__":
    unittest.main()
