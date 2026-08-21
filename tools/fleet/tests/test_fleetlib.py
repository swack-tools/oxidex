#!/usr/bin/env python3
"""Tests for tools/fleet/fleetlib.py.

TWO HUBS, ONE SUITE. By default everything here runs against a throwaway
`git init --bare` repo created in `setUp` -- never the production hub, and
`FleetlibTestCase.setUp` asserts that bare repo's path lives under the
system temp directory before any test body runs.

Set `FLEET_TEST_HUB_URL=<remote>` and the identical suite runs against that
remote instead. That is not a convenience: a local bare repo answers a
`git push` from the filesystem, and the whole Keel migration rests on the
claim that GITHUB honours the same compare-and-swap -- that a non-forced
push to an existing ref is rejected there too, that `--force-with-lease`
guards a delete there too, and that eight racing processes produce exactly
one winner there too. Nothing but a real remote can be evidence for that
claim, so the acceptance run is

    FLEET_TEST_HUB_URL=https://github.com/swack-tools/keel-scratch.git \
        python3 -m unittest test_fleetlib -v

NAMESPACE AND CLEANUP. Every `setUp` mints `refs/fleet/test/<uuid>/` and
every ref a test touches lives under it (`self.ref("one")`). Two
consequences: a live run cannot collide with a previous one that left refs
behind (the old fixed names would have made `create` return False on the
second run and failed a test for the wrong reason), and cleanup is a
namespace sweep rather than a list of names that drifts out of date.
`tearDown` sweeps live namespaces; `tearDownModule` re-sweeps whatever a
crashed test left. Bare-repo runs skip the sweep -- the entire repository
is deleted -- so the default path costs exactly what it always did.

The remote is guarded, not trusted: `_assert_hub_url_is_not_production`
refuses `work2.oxidex.net` and the code repo outright, in BOTH modes.

Plain `unittest`, standard library only (no pytest in this environment --
confirmed via `python3 -m pytest --version` before writing this file).

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import uuid
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import fleetlib  # noqa: E402
from fleetlib import (  # noqa: E402
    Hub,
    HubError,
    HubUnreachableError,
    _is_transport_failure,
    credential_env,
)

# The remote the suite runs against when set; a local bare repo otherwise.
LIVE_HUB_URL = os.environ.get("FLEET_TEST_HUB_URL", "").strip()

# Refused in BOTH modes, environment variable or not. `work2.oxidex.net` is
# the production ssh hub and `.../oxidex.git` is the code repo; this suite
# creates and DELETES refs, so pointing it at either would be destructive.
# The check is on the URL the fixture is about to hand `Hub`, before a
# single git command runs.
_FORBIDDEN_HUB_SUBSTRINGS = ("work2.oxidex.net",)
_FORBIDDEN_HUB_SUFFIXES = ("/oxidex.git", "/oxidex")

# (url, namespace) for every namespace this process created on a live
# remote, so `tearDownModule` can re-sweep what a crashed tearDown left.
_LIVE_NAMESPACES = []


def _git_env(extra=None):
    """`os.environ` plus the fleet credential helper when
    `FLEET_GIT_TOKEN_FILE` is set.

    The raw-git proofs in this file (the ones that deliberately bypass
    `Hub` to show the primitive holds at the git level) must authenticate
    the same way `Hub` does, or they would be untestable against an HTTPS
    remote. When the variable is unset `credential_env` returns a plain
    copy of `os.environ`, so the default path is unchanged.
    """
    env = credential_env()
    env.setdefault("GIT_TERMINAL_PROMPT", "0")
    if extra:
        env.update(extra)
    return env


def _run_git(args, cwd=None, input_bytes=None, env=None):
    return subprocess.run(
        args,
        cwd=cwd,
        input=input_bytes,
        capture_output=True,
        env=_git_env() if env is None else env,
    )


def _sweep(url: str, namespace: str, workdir: str) -> int:
    """Delete every ref under `namespace` on `url`. Returns the count.

    Best effort by construction: a cleanup that raises would mask the test
    failure that caused the leftover. It reports to stderr instead, because
    a leftover ref on a shared remote is the thing that breaks the NEXT
    run.
    """
    removed = 0
    try:
        hub = Hub(url=url, workdir=workdir)
        refs = hub.list(namespace)
    except Exception as exc:  # noqa: BLE001 -- see docstring
        print(f"WARNING: cleanup could not list {namespace}: {exc}", file=sys.stderr)
        return removed
    for ref, sha in sorted(refs.items()):
        try:
            if hub.delete(ref, expect_sha=sha):
                removed += 1
            else:
                print(f"WARNING: cleanup lost a race deleting {ref}", file=sys.stderr)
        except Exception as exc:  # noqa: BLE001
            print(f"WARNING: cleanup could not delete {ref}: {exc}", file=sys.stderr)
    return removed


def tearDownModule():
    """Second sweep, for namespaces a crashed `tearDown` never reached."""
    if not LIVE_HUB_URL:
        return
    leftover = 0
    for url, namespace in _LIVE_NAMESPACES:
        tmp = tempfile.mkdtemp(prefix="fleetlib-sweep-")
        try:
            leftover += _sweep(url, namespace, tmp)
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
    if leftover:
        print(
            f"NOTE: tearDownModule swept {leftover} ref(s) a tearDown missed",
            file=sys.stderr,
        )


class FleetlibTestCase(unittest.TestCase):
    """Base fixture: a throwaway bare repo standing in for the hub, or the
    remote named by `FLEET_TEST_HUB_URL`.

    `self.hub_url` is the remote either way -- it is a filesystem path in
    the default mode and an https URL in live mode, and `git push` accepts
    both, which is why the raw-git proofs below need no second dialect.
    `self.ref("x")` is the only way a test should name a ref.
    """

    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="fleetlib-test-")
        self.workdir = str(Path(self._tmp_root) / "cache")
        self.live = bool(LIVE_HUB_URL)

        # One private namespace per test, in BOTH modes so there is one
        # code path. In the bare mode it is merely unused isolation; on a
        # shared remote it is what makes the suite re-runnable, because
        # `create` of a ref a previous run left behind returns False and
        # would fail a create test for a reason that has nothing to do
        # with the CAS.
        self.ns = f"refs/fleet/test/{uuid.uuid4().hex}/"

        if self.live:
            self.hub_url = LIVE_HUB_URL
            self._assert_hub_url_is_not_production(self.hub_url)
            _LIVE_NAMESPACES.append((self.hub_url, self.ns))
        else:
            self.hub_path = str(Path(self._tmp_root) / "hub.git")
            init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
            self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

            # The single most important assertion in this file: never let
            # the fixture -- or by extension any test -- point at anything
            # but a temp path.
            resolved = str(Path(self.hub_path).resolve())
            system_tmp = str(Path(tempfile.gettempdir()).resolve())
            self.assertTrue(
                resolved.startswith(system_tmp),
                msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
            )
            self._assert_hub_url_is_not_production(resolved)
            self.hub_url = self.hub_path

        self.hub = Hub(url=self.hub_url, workdir=self.workdir)

    def tearDown(self):
        # Order matters: the sweep needs `self.hub`, whose object-store
        # cache lives under `_tmp_root`.
        try:
            if self.live:
                _sweep(self.hub_url, self.ns, self.workdir)
        finally:
            shutil.rmtree(self._tmp_root, ignore_errors=True)

    def _assert_hub_url_is_not_production(self, url: str):
        """The guard, stated once and applied in both modes."""
        for bad in _FORBIDDEN_HUB_SUBSTRINGS:
            self.assertNotIn(
                bad, url, msg=f"test hub {url!r} points at production ({bad})"
            )
        stripped = url.rstrip("/")
        for bad in _FORBIDDEN_HUB_SUFFIXES:
            self.assertFalse(
                stripped.endswith(bad),
                msg=f"test hub {url!r} points at the code repo ({bad})",
            )

    def ref(self, suffix: str) -> str:
        """This test's private ref for `suffix`. The ONLY way a test in
        this file should name a ref: it is what keeps a live run from
        colliding with a previous one and what makes cleanup a sweep.
        """
        return self.ns + suffix

    def fresh_hub(self) -> Hub:
        """A second Hub instance with its own local cache, same remote --
        simulates a second host talking to the same hub.
        """
        other_workdir = tempfile.mkdtemp(prefix="fleetlib-test-cache2-")
        self.addCleanup(shutil.rmtree, other_workdir, ignore_errors=True)
        return Hub(url=self.hub_url, workdir=other_workdir)


class TestFixtureGuard(FleetlibTestCase):
    def test_the_hub_is_never_production(self):
        """True in both modes, and the only assertion that is."""
        self._assert_hub_url_is_not_production(self.hub.url)

    def test_default_mode_hub_url_is_a_temp_path(self):
        if self.live:
            self.skipTest("live mode: the remote is FLEET_TEST_HUB_URL, not a temp path")
        resolved = str(Path(self.hub.url).resolve())
        self.assertTrue(resolved.startswith(str(Path(tempfile.gettempdir()).resolve())))

    def test_live_mode_hub_url_is_exactly_what_the_environment_named(self):
        if not self.live:
            self.skipTest("default mode: no FLEET_TEST_HUB_URL")
        self.assertEqual(self.hub.url, LIVE_HUB_URL)

    def test_every_ref_this_suite_touches_is_inside_its_namespace(self):
        self.assertTrue(self.ns.startswith(self.ns))
        self.assertTrue(self.ns.endswith("/"))
        self.assertTrue(self.ref("x").startswith(self.ns))


class TestCreate(FleetlibTestCase):
    def test_create_then_read(self):
        ok = self.hub.create(self.ref("one"), {"work_key": "abc"})
        self.assertTrue(ok)
        payload = self.hub.read(self.ref("one"))
        self.assertIsNotNone(payload)
        self.assertEqual(payload["work_key"], "abc")
        self.assertEqual(payload["schema_version"], 1)
        self.assertIn("written_by", payload)
        self.assertIn("written_at", payload)

    def test_create_twice_second_fails_first_payload_intact(self):
        first = self.hub.create(self.ref("dup"), {"holder_host": "hostA"})
        self.assertTrue(first)

        second = self.hub.create(self.ref("dup"), {"holder_host": "hostB"})
        self.assertFalse(second)

        payload = self.hub.read(self.ref("dup"))
        self.assertEqual(payload["holder_host"], "hostA", "the second create must not have touched the ref")

    def test_raw_git_nonforced_push_rejects_existing_ref(self):
        """Direct evidence at the git level, bypassing fleetlib entirely,
        that a non-forced `git push <sha>:<ref>` fails closed when the ref
        already exists -- the primitive the whole design rests on.
        """
        version = subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip()

        scratch = tempfile.mkdtemp(prefix="fleetlib-rawgit-")
        self.addCleanup(shutil.rmtree, scratch, ignore_errors=True)
        _run_git(["git", "init", "--quiet", "--bare", scratch])

        def make_commit(git_dir, content):
            blob = _run_git(
                ["git", "--git-dir", git_dir, "hash-object", "-w", "--stdin"],
                input_bytes=content.encode(),
            ).stdout.strip().decode()
            tree = _run_git(
                ["git", "--git-dir", git_dir, "mktree"],
                input_bytes=f"100644 blob {blob}\tpayload.json\n".encode(),
            ).stdout.strip().decode()
            commit = subprocess.run(
                ["git", "--git-dir", git_dir, "commit-tree", tree, "-m", "x"],
                capture_output=True,
                env={
                    **__import__("os").environ,
                    "GIT_AUTHOR_NAME": "t",
                    "GIT_AUTHOR_EMAIL": "t@t",
                    "GIT_COMMITTER_NAME": "t",
                    "GIT_COMMITTER_EMAIL": "t@t",
                },
            ).stdout.strip().decode()
            return commit

        c1 = make_commit(scratch, "{}")
        r1 = _run_git(["git", "--git-dir", scratch, "push", scratch, f"{c1}:refs/fleet/rawtest/x"])
        self.assertEqual(r1.returncode, 0, msg=f"[{version}] first create push failed: {r1.stderr.decode()}")

        c2 = make_commit(scratch, '{"different": true}')
        r2 = _run_git(["git", "--git-dir", scratch, "push", scratch, f"{c2}:refs/fleet/rawtest/x"])
        self.assertNotEqual(
            r2.returncode,
            0,
            msg=f"[{version}] a non-forced push to an existing ref unexpectedly succeeded -- "
            "the CAS primitive this design relies on does not hold for this git version",
        )


def _attempt_create(hub_url: str, ref: str, idx: int):
    """Top-level (picklable) worker for the multiprocessing race test.
    Each worker builds its own Hub with its own local cache dir, so the
    only shared state is the remote -- real inter-process contention on the
    actual CAS primitive, not a mock. Against `FLEET_TEST_HUB_URL` the
    shared state is GitHub itself, which is the point of the live run.
    """
    workdir = tempfile.mkdtemp(prefix=f"fleetlib-race-{idx}-")
    try:
        hub = Hub(url=hub_url, workdir=workdir)
        ok = hub.create(ref, {"claimant_idx": idx})
        return idx, ok
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


class TestConcurrentCreate(FleetlibTestCase):
    """N real OS processes race to create ONE ref; exactly one may win.

    This is the load-bearing test of the whole design, and the reason the
    suite is parametrized over a remote at all: against a local bare repo
    it proves a property of the local filesystem's ref-locking, and against
    `FLEET_TEST_HUB_URL` it proves the same property of GitHub, which is
    the thing the Keel migration actually needs and the thing no amount of
    local testing can establish.

    Two counts, deliberately. Eight is the count PLAN Stage 1's acceptance
    criterion names, so it exists here under that name and can be pointed
    at; twelve is what this test has always run and is not weakened to
    match a document.
    """

    def _race(self, n: int):
        ref = self.ref(f"race{n}")

        with ProcessPoolExecutor(max_workers=n) as pool:
            futures = [pool.submit(_attempt_create, self.hub_url, ref, i) for i in range(n)]
            results = [f.result() for f in as_completed(futures)]

        winners = [idx for idx, ok in results if ok]
        self.assertEqual(
            len(winners),
            1,
            msg=f"expected exactly one winner among {n} real OS-process racers, got {winners}",
        )
        self.assertEqual(
            len(results), n, msg="a racer never reported back -- the race proved nothing"
        )

        payload = self.hub.read(ref)
        self.assertIsNotNone(payload)
        self.assertEqual(
            payload["claimant_idx"],
            winners[0],
            msg="the ref holds a payload from a racer that was told it LOST",
        )

    def test_concurrent_create_exactly_one_winner_of_8_racers(self):
        """PLAN Stage 1 acceptance: 8 racers, exactly one winner."""
        self._race(8)

    def test_concurrent_create_exactly_one_winner(self):
        self._race(12)


class TestUpdate(FleetlibTestCase):
    def test_update_with_correct_expect_sha_succeeds(self):
        self.hub.create(self.ref("upd"), {"v": 1})
        cur_sha = self.hub.sha(self.ref("upd"))
        ok = self.hub.update(self.ref("upd"), {"v": 2}, expect_sha=cur_sha)
        self.assertTrue(ok)
        self.assertEqual(self.hub.read(self.ref("upd"))["v"], 2)

    def test_update_with_stale_expect_sha_fails_ref_unchanged(self):
        self.hub.create(self.ref("upd2"), {"v": 1})
        stale_sha = self.hub.sha(self.ref("upd2"))

        # A legitimate update moves the ref forward...
        ok1 = self.hub.update(self.ref("upd2"), {"v": 2}, expect_sha=stale_sha)
        self.assertTrue(ok1)
        current_sha_after = self.hub.sha(self.ref("upd2"))

        # ...so a second writer still holding the old sha must lose.
        ok2 = self.hub.update(self.ref("upd2"), {"v": 99}, expect_sha=stale_sha)
        self.assertFalse(ok2)

        self.assertEqual(self.hub.sha(self.ref("upd2")), current_sha_after)
        self.assertEqual(self.hub.read(self.ref("upd2"))["v"], 2)


class TestDelete(FleetlibTestCase):
    def test_delete_with_correct_expect_sha_succeeds(self):
        self.hub.create(self.ref("del"), {"v": 1})
        cur_sha = self.hub.sha(self.ref("del"))
        ok = self.hub.delete(self.ref("del"), expect_sha=cur_sha)
        self.assertTrue(ok)
        self.assertIsNone(self.hub.read(self.ref("del")))
        self.assertIsNone(self.hub.sha(self.ref("del")))

    def test_delete_with_stale_expect_sha_fails_ref_unchanged(self):
        self.hub.create(self.ref("del2"), {"v": 1})
        stale_sha = self.hub.sha(self.ref("del2"))

        self.hub.update(self.ref("del2"), {"v": 2}, expect_sha=stale_sha)
        current_sha = self.hub.sha(self.ref("del2"))

        ok = self.hub.delete(self.ref("del2"), expect_sha=stale_sha)
        self.assertFalse(ok)
        self.assertEqual(self.hub.sha(self.ref("del2")), current_sha)
        self.assertIsNotNone(self.hub.read(self.ref("del2")))

    def test_raw_git_force_with_lease_on_delete(self):
        """Direct evidence that `--force-with-lease=<ref>:<sha>` actually
        guards a *deletion*, not just an update, for this git version. The
        spec explicitly asks this not be assumed.
        """
        version = subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip()

        rawdel = self.ref("rawdel")
        self.hub.create(rawdel, {"v": 1})
        real_sha = self.hub.sha(rawdel)
        wrong_sha = "0" * 40

        # Stale-lease delete must be rejected, not silently accepted as an
        # unconditional force.
        r_bad = _run_git(
            [
                "git",
                "--git-dir",
                self.workdir,
                "push",
                f"--force-with-lease={rawdel}:{wrong_sha}",
                self.hub_url,
                f":{rawdel}",
            ]
        )
        self.assertNotEqual(
            r_bad.returncode,
            0,
            msg=f"[{version}] --force-with-lease delete with a wrong expected sha unexpectedly succeeded",
        )
        self.assertIsNotNone(self.hub.sha(self.ref("rawdel")))

        # Correct-lease delete must succeed.
        r_good = _run_git(
            [
                "git",
                "--git-dir",
                self.workdir,
                "push",
                f"--force-with-lease={rawdel}:{real_sha}",
                self.hub_url,
                f":{rawdel}",
            ]
        )
        self.assertEqual(
            r_good.returncode,
            0,
            msg=f"[{version}] --force-with-lease delete with the correct expected sha failed: "
            f"{r_good.stderr.decode()}",
        )
        self.assertIsNone(self.hub.sha(self.ref("rawdel")))


class TestRead(FleetlibTestCase):
    def test_read_of_absent_ref_is_none_not_exception(self):
        self.assertIsNone(self.hub.read(self.ref("does-not-exist")))
        self.assertIsNone(self.hub.sha(self.ref("does-not-exist")))

    def test_payload_roundtrips_unicode_and_nested_dicts(self):
        payload = {
            "title": "Route legacy formats — SWF, PICT, PPM, RA, 京セラ RAW 🎯",
            "scope": {
                "formats": ["SWF", "PICT", "PPM", "RA", "KyoceraRAW"],
                "nested": {"deeper": {"deepest": ["a", 1, None, True, 3.14]}},
            },
            "note": "quotes \" and backslashes \\ and newlines\nshould survive",
        }
        self.hub.create(self.ref("unicode"), payload)
        got = self.hub.read(self.ref("unicode"))
        for key, value in payload.items():
            self.assertEqual(got[key], value)


class TestReadIsCoherentUnderConcurrentWrites(FleetlibTestCase):
    """`Hub.read`'s time-of-check/time-of-use race, and its fix.

    THE DEFECT (found by `tools/fleet/tests/test_seams.py`, seam 7, during
    an unrelated seam-2 run -- not by inspection):

        found_sha = self._remote_sha(ref)              # ls-remote -> S1
        self._run(["fetch", ..., f"+{ref}:{tmp_ref}"]) # brings S2
        self._run(["cat-file", "-p", f"{found_sha}:payload.json"])

    A ref that moved between the ls-remote and the fetch left the fetch
    bringing S2 while the cat-file asked for S1. Payload commits are
    orphans, so nothing else drags S1 into the local object store, and git
    answers `fatal: path 'payload.json' does not exist in '<S1>'` -- which
    `Hub.read` raised as `HubError`. `fleetd.reconcile_once` had no `try`
    around the `hub.read` calls that raised it, so a claim renewing
    underneath a queue computation could kill the daemon.

    The window is not exotic: renewing leases rewrite every held claim ref
    once per renewal interval, and the queue reads every claim payload on
    every loop.

    NAME THE INSTRUMENT. The move is FORCED, by patching `_remote_sha` on
    the reader's own Hub instance, not raced -- a test that reproduces a
    race one run in twenty is not a regression test. Every write is a real
    `git push` to the real fixture bare repo (`FleetlibTestCase.setUp`,
    asserted to be under the system temp dir); nothing here is mocked
    except the moment the interleaving happens.
    """

    # Per-test, not a class attribute: the namespace is minted in setUp,
    # so the ref this class works on cannot be known at class-body time.
    def setUp(self):
        super().setUp()
        self.REF = self.ref("racy")

    def _move_ref_during_ls_remote(self, reader: Hub, new_payload: dict):
        """Patch `reader._remote_sha` so the FIRST resolution of `self.REF`
        also advances the ref on the hub -- exactly the production
        interleaving, made deterministic. Returns the list the writer's
        successes are recorded in.
        """
        writer = self.fresh_hub()
        real = reader._remote_sha
        moved = []

        def sha_then_move(ref):
            found = real(ref)
            if ref == self.REF and not moved:
                moved.append(writer.update(ref, new_payload, expect_sha=found))
            return found

        reader._remote_sha = sha_then_move
        self.addCleanup(lambda: setattr(reader, "_remote_sha", real))
        return moved

    def test_read_survives_a_ref_that_moves_between_ls_remote_and_fetch(self):
        self.assertTrue(self.hub.create(self.REF, {"v": 1, "holder": "a"}))
        reader = self.fresh_hub()  # never read this ref before: empty object store
        moved = self._move_ref_during_ls_remote(reader, {"v": 2, "holder": "a"})

        payload = reader.read(self.REF)

        self.assertTrue(moved and moved[0], "the forced concurrent write did not land")
        self.assertIsNotNone(payload, "read raised or returned None for a ref that exists")
        self.assertEqual(
            payload["v"], 2,
            "read must return the payload the fetch actually brought, not a stale one",
        )

    def test_read_with_sha_returns_a_sha_that_matches_its_payload(self):
        self.assertTrue(self.hub.create(self.REF, {"v": 1, "holder": "a"}))
        reader = self.fresh_hub()
        moved = self._move_ref_during_ls_remote(reader, {"v": 2, "holder": "a"})

        got_sha, payload = reader.read_with_sha(self.REF)

        self.assertTrue(moved and moved[0], "the forced concurrent write did not land")
        self.assertIsNotNone(got_sha)
        self.assertEqual(payload["v"], 2)
        # Coherence is the whole point: the sha handed back is the one whose
        # payload was handed back. A `sha()` + `read()` pair cannot promise
        # that, which is why this method exists.
        self.assertEqual(
            got_sha, self.hub.sha(self.REF),
            "read_with_sha returned a sha that is not the one it read the payload from",
        )

    def test_a_ref_deleted_inside_the_window_reads_as_absent_not_an_error(self):
        """The other direction: absence must stay absence. A ref that is
        DELETED between the ls-remote and the fetch is a legitimate `None`,
        never a raise -- callers distinguish "nobody claimed this" from
        "the hub is down" by exactly that.
        """
        self.assertTrue(self.hub.create(self.REF, {"v": 1}))
        reader = self.fresh_hub()
        deleter = self.fresh_hub()
        real = reader._remote_sha
        deleted = []

        def sha_then_delete(ref):
            found = real(ref)
            if ref == self.REF and not deleted:
                deleted.append(deleter.delete(ref, expect_sha=found))
            return found

        reader._remote_sha = sha_then_delete
        self.addCleanup(lambda: setattr(reader, "_remote_sha", real))

        payload = reader.read(self.REF)

        self.assertTrue(deleted and deleted[0], "the forced concurrent delete did not land")
        self.assertIsNone(payload)

    def test_absent_and_unreachable_stay_distinguishable(self):
        """Regression guard on the module's central promise, restated for
        `read_with_sha`: absent is `(None, None)`, unreachable RAISES.
        """
        self.assertEqual((None, None), self.hub.read_with_sha(self.ref("nope")))
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus-coh-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.read_with_sha(self.ref("whatever"))

    def test_a_commit_with_no_payload_still_raises_and_is_not_retried_away(self):
        """The fix must not turn a genuinely malformed ref into a silent
        pass. A ref pointing at a commit whose tree has no `payload.json`
        is a real error -- retrying cannot change it, and `read` must say
        so with the wording the seam suite matches on.
        """
        git = ["git", "--git-dir", self.workdir]
        env_commit = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
                      "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        empty_tree = _run_git(git + ["hash-object", "-t", "tree", "-w", "--stdin"], input_bytes=b"")
        self.assertEqual(empty_tree.returncode, 0, msg=empty_tree.stderr.decode())
        tree_sha = empty_tree.stdout.decode().strip()
        commit = subprocess.run(git + ["commit-tree", tree_sha, "-m", "no payload"],
                                capture_output=True, env={**os.environ, **env_commit})
        self.assertEqual(commit.returncode, 0, msg=commit.stderr.decode())
        commit_sha = commit.stdout.decode().strip()
        push = _run_git(git + ["push", self.hub_url, f"{commit_sha}:{self.ref('empty')}"])
        self.assertEqual(push.returncode, 0, msg=push.stderr.decode())

        with self.assertRaises(HubError) as ctx:
            self.fresh_hub().read(self.ref("empty"))
        self.assertIn("has no readable payload.json", str(ctx.exception))
        self.assertIn(commit_sha, str(ctx.exception),
                      "the error must name the sha actually read, not '<unresolved>'")


class TestFetchFailureClassification(FleetlibTestCase):
    """The second half of the absent/unreachable promise: what `_read`
    does when the `ls-remote` SUCCEEDS and the `fetch` then fails.

    THE GAP THIS CLOSES. `TestReadIsCoherentUnderConcurrentWrites` covers
    the ref MOVING and the ref being DELETED inside that window. Nothing
    covered the third thing that can happen there -- the TRANSPORT dying
    between the two commands -- and the code classified it like this:

        if "couldn't find remote ref" in low or "not found" in low:
            return None, None                      # absent
        raise HubUnreachableError(...)

    `fatal: repository 'ssh://hub/oxidex.git' not found` -- git's wording
    for the hub itself being gone -- contains "not found". So the single
    most consequential transport failure in the system returned None, and
    None from `read()` reads as "nobody has claimed this yet". Every
    claim on the hub appeared unheld at exactly the moment the hub could
    no longer arbitrate; `claim.acquire` invites a second host onto work
    that is already running, which is the double-claim leases exist to
    prevent.

    The ls-remote guarding step (1) does not save this: it answers the
    absence question for its own instant only, and the transport can die
    after it returns. This branch is the only place a fetch failure is
    ever classified.

    NAME THE INSTRUMENT. The hub is the real fixture bare repo and the
    `ls-remote` really succeeds against it -- only `git fetch` is
    intercepted, on the reader's own Hub instance, and it answers with
    git's verbatim stderr for each failure mode. `read()` is exercised
    (not just `read_with_sha`) because `read()` is the one whose None the
    fleet reads as absence.
    """

    def reader_whose_fetch_fails(self, returncode: int, stderr: str) -> Hub:
        """A Hub whose `ls-remote` is real and whose `fetch` fails with
        `stderr`. Returns the Hub; the ref exists on the hub, so step (1)
        resolves it and the failure lands squarely on the fetch."""
        from fleetlib import _Result

        reader = self.fresh_hub()
        real_run = reader._run

        def only_fetch_fails(args, **kw):
            if args and args[0] == "fetch":
                return _Result(returncode, "", stderr, list(args))
            return real_run(args, **kw)

        reader._run = only_fetch_fails
        return reader

    def setUp(self):
        super().setUp()
        self.REF = self.ref("classify")
        self.assertTrue(self.hub.create(self.REF, {"holder_host": "somebody", "pgid": 111}))

    def test_a_transport_failure_at_the_fetch_raises_and_is_never_absence(self):
        """THE REGRESSION TEST. ls-remote succeeds, fetch reports the hub
        is gone. That is unreachable, and unreachable RAISES."""
        reader = self.reader_whose_fetch_fails(
            128, "fatal: repository 'ssh://hub.example/oxidex.git' not found\n"
        )
        with self.assertRaises(HubUnreachableError) as ctx:
            reader.read(self.REF)
        self.assertIn("fetch of", str(ctx.exception))

    def test_every_transport_class_message_raises_rather_than_returning_none(self):
        """One test per way the transport dies, because the defect was a
        substring match and a substring match fails per-message. Each of
        these is git's real stderr, and not one of them means the ref is
        absent -- the ref demonstrably exists (setUp created it).
        """
        messages = (
            "fatal: repository 'ssh://hub.example/oxidex.git' not found",
            "ssh: Could not resolve hostname hub.example: nodename nor servname provided",
            "fatal: Could not read from remote repository.",
            "ssh: connect to host hub.example port 2244: Connection refused",
            "fatal: '/gone/hub.git' does not appear to be a git repository",
            "fatal: unable to access 'https://hub.example/oxidex.git/': Could not resolve host",
            "fatal: the remote end hung up unexpectedly",
            "Permission denied (publickey).",
            "fatal: early EOF",
        )
        for msg in messages:
            with self.subTest(stderr=msg):
                reader = self.reader_whose_fetch_fails(128, msg + "\n")
                with self.assertRaises(HubUnreachableError):
                    reader.read(self.REF)

    def test_an_unrecognized_fetch_failure_fails_closed(self):
        """Not every future git message is in the hint list. A fetch
        failure nobody has classified must RAISE, not invent an absence:
        returning None is a positive claim about the hub's contents, and
        an unrecognized error is not evidence for one.
        """
        reader = self.reader_whose_fetch_fails(1, "fatal: something nobody has seen before\n")
        with self.assertRaises(HubUnreachableError):
            reader.read(self.REF)

    def test_the_one_real_absence_signature_still_reads_as_absent(self):
        """The other direction, and the reason the fix is a NARROWING
        rather than a deletion: a ref deleted between the ls-remote and
        the fetch is a legitimate None. `couldn't find remote ref` is the
        only fetch stderr that means that.
        """
        reader = self.reader_whose_fetch_fails(
            128, f"fatal: couldn't find remote ref {self.REF}\n"
        )
        self.assertIsNone(reader.read(self.REF))
        self.assertEqual((None, None), self.reader_whose_fetch_fails(
            128, f"fatal: couldn't find remote ref {self.REF}\n"
        ).read_with_sha(self.REF))

    def test_a_message_carrying_both_signatures_is_classified_as_transport(self):
        """ORDER, stated as a test. `fatal: repository '<url>' not found`
        already proves the ordering matters for the two words "not found";
        this pins the general rule for any message that carries a
        transport hint AND the absence phrase. Transport wins: calling an
        unreachable hub "empty" is the expensive direction of being wrong,
        and only one of the two mistakes double-claims a branch.
        """
        reader = self.reader_whose_fetch_fails(
            128,
            f"fatal: couldn't find remote ref {self.REF}\n"
            "fatal: Could not read from remote repository.\n",
        )
        with self.assertRaises(HubUnreachableError):
            reader.read(self.REF)


class TestList(FleetlibTestCase):
    def test_list_returns_matching_refs_only(self):
        self.hub.create(self.ref("claims/gate/aaa"), {"k": 1})
        self.hub.create(self.ref("claims/gate/bbb"), {"k": 2})
        self.hub.create(self.ref("claims/agent/ccc"), {"k": 3})
        self.hub.create(self.ref("verdicts/ddd"), {"k": 4})

        gate_claims = self.hub.list(self.ref("claims/gate"))
        self.assertEqual(set(gate_claims.keys()), {self.ref("claims/gate/aaa"), self.ref("claims/gate/bbb")})

        all_claims = self.hub.list(self.ref("claims"))
        self.assertEqual(
            set(all_claims.keys()),
            {self.ref("claims/gate/aaa"), self.ref("claims/gate/bbb"), self.ref("claims/agent/ccc")},
        )


class TestUnreachable(FleetlibTestCase):
    def test_unreachable_hub_raises_on_sha(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.sha(self.ref("whatever"))

    def test_unreachable_hub_raises_on_read_not_none(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus2-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.read(self.ref("whatever"))

    def test_unreachable_hub_raises_on_create(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus3-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.create(self.ref("whatever"), {"x": 1})

    def test_unreachable_hub_raises_on_list(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus4-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.list(self.ns)



class TestCodeUrl(FleetlibTestCase):
    """`Hub.code_url` -- the one attribute the borrowers change to (SPEC 4.4).

    The whole value of this attribute is its DEFAULT. Coordination state is
    moving to a private repo while the code stays public, and three modules
    (`workqueue._fetch_for_ancestry`, `dispatch._have_objects`,
    `train._fetch_into_hub_cache`) currently borrow `hub.url` to answer code
    questions. If `code_url` did not default to `url`, every one of those
    call sites and every fixture in this tree would have to change at once.
    Because it does, they change one line each and the local
    `git init --bare` fixtures keep working untouched -- which is what
    `test_queue`, `test_dispatch` and `test_train` staying green proves.
    """

    def test_code_url_defaults_to_url(self):
        self.assertEqual(self.hub.code_url, self.hub.url)

    def test_code_url_can_differ_and_does_not_disturb_url(self):
        other = str(Path(self._tmp_root) / "code.git")
        self.assertEqual(
            _run_git(["git", "init", "--quiet", "--bare", other]).returncode, 0
        )
        split = Hub(
            url=self.hub_url,
            workdir=str(Path(self._tmp_root) / "cache-split"),
            code_url=other,
        )
        self.assertEqual(split.url, self.hub_url)
        self.assertEqual(split.code_url, other)
        self.assertNotEqual(split.code_url, split.url)

    def test_coordination_writes_still_go_to_url_not_code_url(self):
        """The split must not move a single CAS write. `create` against a
        Hub whose `code_url` is somewhere else lands on `url`, and the other
        repo stays empty -- otherwise the private/public split would leak
        claims (`user@host:pid`, paths, hostnames) into the PUBLIC repo,
        which is the exposure SPEC 8 exists to prevent.
        """
        other = str(Path(self._tmp_root) / "code2.git")
        self.assertEqual(
            _run_git(["git", "init", "--quiet", "--bare", other]).returncode, 0
        )
        split = Hub(
            url=self.hub_url,
            workdir=str(Path(self._tmp_root) / "cache-split2"),
            code_url=other,
        )
        self.assertTrue(split.create(self.ref("split"), {"secret": "hostname:pid"}))
        self.assertIsNotNone(self.hub.sha(self.ref("split")))

        code_side = Hub(url=other, workdir=str(Path(self._tmp_root) / "cache-split3"))
        self.assertEqual({}, code_side.list("refs/fleet"))

    def test_code_url_is_resolved_once_not_aliased(self):
        """Reassigning `url` afterwards must not drag `code_url` with it:
        `FallbackHub` presents the GitHub half's `.url`/`.code_url` as its
        own, and a live alias would make that presentation ambiguous.
        """
        hub = Hub(url=self.hub_url, workdir=str(Path(self._tmp_root) / "cache-alias"))
        self.assertEqual(hub.code_url, self.hub_url)
        hub.url = "/somewhere/else.git"
        self.assertEqual(hub.code_url, self.hub_url)


class TestFetchNamespace(FleetlibTestCase):
    """`Hub.fetch_namespace(prefix)` -- one round trip, whole namespace.

    WHY THE UNION OF TWO PATTERNS, given git will not let both shapes exist
    at once. Measured here, not assumed: `create("<p>")` followed by
    `create("<p>/child")` returns False, because git's ref store refuses a
    ref and a directory of the same name (`cannot lock ref ...: '<p>'
    exists`). So a prefix is EITHER a leaf OR a directory, never both --
    and that is exactly why the union is the right read. The caller of a
    namespace read does not know which one it has: `refs/fleet/signals/tip`
    is a leaf, `refs/fleet/claims` is a directory, and a server building an
    index over `refs/fleet/*` walks both. `list()` only ever asks the
    directory question, so it reports the tip signal -- a ref that exists,
    that the train CAS-bumps every time the tip advances -- as absent. One
    `ls-remote` carrying both patterns answers correctly either way.
    """

    def _ls_remote_counting_hub(self):
        """A Hub that records how many `ls-remote` invocations it makes.
        ONE is the promised cost, and against GitHub at ~0.6 s a call the
        difference between one and N is the difference between a server
        index build that is instant and one that is not.
        """
        hub = self.fresh_hub()
        calls = []
        real_run = hub._run

        def counting(args, **kw):
            if args and args[0] == "ls-remote":
                calls.append(list(args))
            return real_run(args, **kw)

        hub._run = counting
        return hub, calls

    def test_a_prefix_that_is_a_leaf_returns_that_leaf(self):
        """`refs/fleet/signals/tip` in miniature -- the case `list()` gets
        wrong.
        """
        self.assertTrue(self.hub.create(self.ref("sig"), {"generation": 1}))
        got = self.hub.fetch_namespace(self.ref("sig"))
        self.assertEqual(set(got), {self.ref("sig")})
        self.assertEqual(got[self.ref("sig")], self.hub.sha(self.ref("sig")))

    def test_a_prefix_that_is_a_directory_returns_the_whole_subtree(self):
        self.assertTrue(self.hub.create(self.ref("ns/a"), {"k": "a"}))
        self.assertTrue(self.hub.create(self.ref("ns/deep/deeper"), {"k": "b"}))
        self.assertTrue(self.hub.create(self.ref("sibling"), {"k": "c"}))

        got = self.hub.fetch_namespace(self.ref("ns"))

        self.assertEqual(set(got), {self.ref("ns/a"), self.ref("ns/deep/deeper")})
        for ref, sha in got.items():
            self.assertEqual(sha, self.hub.sha(ref), msg=f"wrong sha for {ref}")

    def test_the_leaf_at_the_prefix_is_exactly_what_list_misses(self):
        """The reason this method exists rather than being an alias for
        `list`. A namespace read that drops a leaf is a namespace read that
        lies, and the leaf it would drop is the tip signal the train bumps
        on every advance.
        """
        self.assertTrue(self.hub.create(self.ref("sig"), {"generation": 7}))

        self.assertEqual({}, self.hub.list(self.ref("sig")))
        self.assertEqual(
            {self.ref("sig")}, set(self.hub.fetch_namespace(self.ref("sig")))
        )

    def test_git_refuses_a_leaf_and_a_directory_of_the_same_name(self):
        """The measurement the docstring above rests on, pinned so nobody
        has to re-derive it -- and a live claim about `create`'s contract:
        a directory/file conflict is reported as a LOST RACE (False), not
        as a transport failure. "Something is already in the way" is what
        False has always meant here, and this is a genuine instance of it.
        """
        self.assertTrue(self.hub.create(self.ref("leaf"), {"k": 1}))
        self.assertFalse(self.hub.create(self.ref("leaf/child"), {"k": 2}))
        self.assertIsNone(self.hub.sha(self.ref("leaf/child")))

    def test_it_is_exactly_one_ls_remote(self):
        self.assertTrue(self.hub.create(self.ref("ns/a"), {"k": 1}))
        self.assertTrue(self.hub.create(self.ref("ns/b"), {"k": 2}))

        hub, calls = self._ls_remote_counting_hub()
        got = hub.fetch_namespace(self.ref("ns"))

        self.assertEqual(len(got), 2)
        self.assertEqual(
            len(calls), 1, msg=f"expected exactly one ls-remote, got {calls}"
        )
        self.assertEqual(
            calls[0][2:],
            [self.ref("ns"), self.ref("ns") + "/*"],
            msg="the single ls-remote must carry both the exact and the subtree pattern",
        )

    def test_trailing_slash_and_star_are_the_same_namespace(self):
        self.assertTrue(self.hub.create(self.ref("norm/x"), {"k": 1}))
        self.assertTrue(self.hub.create(self.ref("norm/y"), {"k": 2}))
        base = self.hub.fetch_namespace(self.ref("norm"))
        self.assertEqual(2, len(base))
        self.assertEqual(base, self.hub.fetch_namespace(self.ref("norm") + "/"))
        self.assertEqual(base, self.hub.fetch_namespace(self.ref("norm") + "/*"))
        self.assertEqual(base, self.hub.fetch_namespace(self.ref("norm") + "*"))

    def test_an_empty_namespace_is_an_empty_dict(self):
        self.assertEqual({}, self.hub.fetch_namespace(self.ref("nothing-here")))

    def test_an_empty_prefix_is_refused(self):
        """A namespace read with no namespace would return every ref on the
        spine; that is never what a caller meant, so it is an error rather
        than a very expensive surprise.
        """
        for bad in ("", "   ", "/", "*", "/*"):
            with self.subTest(prefix=bad):
                with self.assertRaises(ValueError):
                    self.hub.fetch_namespace(bad)

    def test_unreachable_raises_and_is_never_an_empty_namespace(self):
        """The module's central promise, restated for the namespace read:
        `{}` is a positive claim that the namespace is empty, and an
        unreachable spine is not evidence for one. An index build that
        treats an outage as "no claims exist" hands every held claim back
        out to a second host.
        """
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus-ns-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(
            url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir
        )
        with self.assertRaises(HubUnreachableError):
            bogus.fetch_namespace("refs/fleet/claims")


class TestGitHubRateLimitsAreTransient(FleetlibTestCase):
    """A rate limit is a "not now", and "not now" must never read as "you
    lost the race".

    THE DEFECT THIS PREVENTS. On an ssh hub a busy remote drops the
    connection; on GitHub it answers, politely, with a REJECTION:

        remote: You have exceeded a secondary rate limit ...
         ! [remote rejected] <sha> -> refs/fleet/claims/gate/x (...)
        error: failed to push some refs to '<url>'

    `[rejected]` is a `_PUSH_REJECTION_PATTERNS` entry, so read
    content-first that is indistinguishable from a lost CAS race, and both
    lies it tells are silent and expensive: `create` -> False stands a
    healthy host down from work nobody is doing, and `update` -> False is
    what `claim._mark_lost` turns into a killed healthy gate (claim.py
    L677-682, and `fleetd` kills by process group). The fix is the ordering
    rule this module already applies to fetch failures -- transport is
    consulted FIRST -- plus the three GitHub wordings in `_TRANSPORT_HINTS`.

    NAME THE INSTRUMENT. The stderr strings below are GitHub's own wording;
    the `_Result`s are synthesized so the classification is tested directly
    rather than by provoking a real rate limit, and the fetch case drives
    the real `_read` path against the real fixture remote with only `git
    fetch` intercepted.
    """

    RATE_LIMITED = (
        "remote: You have exceeded a secondary rate limit and have been "
        "temporarily blocked from content creation.",
        "remote: You have triggered an abuse detection mechanism.",
        "fatal: unable to access 'https://github.com/x/y.git/': "
        "API rate limit exceeded for user ID 1.",
    )

    def test_each_wording_classifies_as_transport(self):
        for msg in self.RATE_LIMITED:
            with self.subTest(stderr=msg):
                self.assertTrue(_is_transport_failure(msg))

    def test_a_rate_limited_push_raises_rather_than_reporting_a_lost_race(self):
        from fleetlib import _Result

        stderr = (
            "remote: You have exceeded a secondary rate limit and have been "
            "temporarily blocked from content creation.\n"
            "remote: Please retry your request again later.\n"
            "To https://github.com/swack-tools/oxidex-fleet-state.git\n"
            " ! [remote rejected] abc -> refs/fleet/claims/gate/x (rate limited)\n"
            "error: failed to push some refs\n"
        )
        result = _Result(returncode=1, stdout="", stderr=stderr, args=["push"])
        with self.assertRaises(HubUnreachableError):
            self.hub._interpret_push(result)

    def test_a_plain_lost_race_still_returns_false(self):
        """The negative control, and the reason the change is an ORDERING
        rather than a deletion. git's ordinary CAS rejections carry no
        transport hint, so they still mean exactly what they meant.
        """
        from fleetlib import _Result

        for stderr in (
            "To /tmp/hub.git\n ! [rejected] abc -> refs/fleet/x (stale info)\n"
            "error: failed to push some refs to '/tmp/hub.git'\n",
            "To /tmp/hub.git\n ! [rejected] abc -> refs/fleet/x (non-fast-forward)\n",
            "error: failed to update ref\n",
        ):
            with self.subTest(stderr=stderr):
                result = _Result(returncode=1, stdout="", stderr=stderr, args=["push"])
                self.assertFalse(self.hub._interpret_push(result))

    def test_a_real_losing_create_still_returns_false_end_to_end(self):
        """Belt and braces against the ordering change: the live CAS path,
        against the real remote, still reports a lost race as False.
        """
        self.assertTrue(self.hub.create(self.ref("still-false"), {"who": "a"}))
        self.assertFalse(self.fresh_hub().create(self.ref("still-false"), {"who": "b"}))

    def test_a_rate_limited_fetch_raises_rather_than_reading_as_absent(self):
        from fleetlib import _Result

        self.assertTrue(self.hub.create(self.ref("rl"), {"holder_host": "somebody"}))
        reader = self.fresh_hub()
        real_run = reader._run

        def only_fetch_fails(args, **kw):
            if args and args[0] == "fetch":
                return _Result(
                    128,
                    "",
                    "remote: You have triggered an abuse detection mechanism.\n",
                    list(args),
                )
            return real_run(args, **kw)

        reader._run = only_fetch_fails
        with self.assertRaises(HubUnreachableError):
            reader.read(self.ref("rl"))


class TestCredentialHelper(unittest.TestCase):
    """`FLEET_GIT_TOKEN_FILE` -> `tools/fleet/keel/git-credential-file`.

    No hub is needed for any of this: the instrument is `git credential
    fill`, which is git's own way of asking "what credential would you use
    here", so these tests prove the whole chain -- environment, config,
    helper, token file, `password=` on git's stdin -- without a network.
    """

    TOKEN = "ghp_TESTTOKEN_not_a_real_credential_0123456789"

    def setUp(self):
        self._tmp = tempfile.mkdtemp(prefix="fleetlib-cred-")
        self.addCleanup(shutil.rmtree, self._tmp, ignore_errors=True)
        self.token_file = str(Path(self._tmp) / "github.token")
        Path(self.token_file).write_text(self.TOKEN + "\n")
        os.chmod(self.token_file, 0o600)

    @staticmethod
    def _fill(request=b"protocol=https\nhost=github.com\n\n"):
        return Hub._raw_run(["git", "credential", "fill"], input=request)

    # -- the "unset changes nothing" property ---------------------------

    def test_without_the_variable_the_environment_is_untouched(self):
        """The property the entire existing suite depends on. Every ssh-hub
        caller, every `git init --bare` fixture and every test in this tree
        must see the exact git invocation it saw before the helper existed.
        """
        base = {"PATH": "/usr/bin", "HOME": "/home/nobody"}
        self.assertEqual(base, credential_env(base))
        self.assertNotIn("GIT_CONFIG_COUNT", credential_env(base))

    def test_the_returned_env_is_a_copy_not_the_input(self):
        base = {"PATH": "/usr/bin"}
        out = credential_env(base)
        out["MUTATED"] = "1"
        self.assertNotIn("MUTATED", base)

    # -- the helper answers, and answers from the file ------------------

    def test_the_helper_script_ships_and_is_executable(self):
        helper = Path(fleetlib.__file__).resolve().parent / "keel" / "git-credential-file"
        self.assertTrue(helper.is_file(), msg=f"{helper} is missing")
        self.assertTrue(os.access(helper, os.X_OK), msg=f"{helper} is not executable")

    def test_git_gets_the_token_from_the_file(self):
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": self.token_file}):
            result = self._fill()
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        self.assertIn(f"password={self.TOKEN}", result.stdout)
        self.assertIn("username=x-access-token", result.stdout)

    def test_the_username_is_overridable(self):
        with mock.patch.dict(
            os.environ,
            {"FLEET_GIT_TOKEN_FILE": self.token_file, "FLEET_GIT_TOKEN_USER": "keel"},
        ):
            result = self._fill()
        self.assertIn("username=keel", result.stdout)

    def test_only_the_first_line_of_the_token_file_is_used(self):
        Path(self.token_file).write_text(self.TOKEN + "\n# operator note\n")
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": self.token_file}):
            result = self._fill()
        self.assertIn(f"password={self.TOKEN}\n", result.stdout)
        self.assertNotIn("operator note", result.stdout)

    def test_a_host_configured_helper_cannot_win(self):
        """THE REASON THERE ARE TWO CONFIG ENTRIES. A helper already
        configured in ~/.gitconfig -- osxkeychain on these laptops, and the
        1Password agent this fleet has been bitten by before -- runs first
        and answers with whatever the human logged in as, not the host's
        scoped PAT. The empty-valued `credential.helper` resets the list,
        so ours is the only one left.

        The decoy is staged the same way a host helper would be, as a
        GIT_CONFIG_* pair AHEAD of ours, and it would answer with a
        different password if it were ever consulted.
        """
        decoy = {
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "credential.helper",
            "GIT_CONFIG_VALUE_0": "!printf 'username=decoy\\npassword=DECOY-WINS\\n'",
            "FLEET_GIT_TOKEN_FILE": self.token_file,
        }
        env = credential_env({**os.environ, **decoy})
        # The decoy survives (we append, never overwrite)...
        self.assertEqual(env["GIT_CONFIG_VALUE_0"], decoy["GIT_CONFIG_VALUE_0"])
        self.assertEqual(env["GIT_CONFIG_COUNT"], "3")
        # ...and the reset entry is the one that makes it lose.
        self.assertEqual(env["GIT_CONFIG_KEY_1"], "credential.helper")
        self.assertEqual(env["GIT_CONFIG_VALUE_1"], "")

        with mock.patch.dict(os.environ, decoy):
            result = self._fill()
        self.assertNotIn("DECOY-WINS", result.stdout)
        self.assertIn(f"password={self.TOKEN}", result.stdout)

    # -- the token never leaves the pipe --------------------------------

    def test_the_token_is_never_in_an_environment_value_or_an_argument(self):
        """The whole reason this is a helper and not `http.extraHeader` or
        a token-in-URL remote: `fleetd._ps_env` reads other processes'
        environments by design, and `ps -eo args` shows argv to every user
        on the host.
        """
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": self.token_file}):
            env = credential_env()
        for key, value in env.items():
            if key == "FLEET_GIT_TOKEN_FILE":
                continue
            self.assertNotIn(self.TOKEN, str(value), msg=f"token leaked into ${key}")

    def test_a_failing_git_command_does_not_echo_the_token(self):
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": self.token_file}):
            result = Hub._raw_run(["git", "credential", "fill"], input=b"protocol=https\n")
        self.assertNotIn(self.TOKEN, result.stderr)
        self.assertNotIn(self.TOKEN, " ".join(result.args))

    # -- refusals, all of them silent to git ----------------------------

    def _helper(self, op, request, extra_env=None):
        helper = Path(fleetlib.__file__).resolve().parent / "keel" / "git-credential-file"
        return subprocess.run(
            [str(helper), op],
            input=request.encode(),
            capture_output=True,
            env={**os.environ, "FLEET_GIT_TOKEN_FILE": self.token_file, **(extra_env or {})},
        )

    def test_store_and_erase_are_silent_no_ops(self):
        """A helper that cannot store cannot cache a token anywhere the
        operator did not put it.
        """
        for op in ("store", "erase"):
            with self.subTest(op=op):
                r = self._helper(
                    op, f"protocol=https\nhost=github.com\npassword={self.TOKEN}\n\n"
                )
                self.assertEqual(r.returncode, 0)
                self.assertEqual(r.stdout, b"")

    def test_plain_http_is_refused(self):
        r = self._helper("get", "protocol=http\nhost=github.com\n\n")
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout, b"")

    def test_an_unknown_operation_is_refused_quietly(self):
        r = self._helper("nonsense", "protocol=https\nhost=github.com\n\n")
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout, b"")

    def test_an_empty_token_file_supplies_nothing(self):
        empty = str(Path(self._tmp) / "empty.token")
        Path(empty).write_text("\n")
        r = self._helper(
            "get", "protocol=https\nhost=github.com\n\n", {"FLEET_GIT_TOKEN_FILE": empty}
        )
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout, b"")
        self.assertNotIn(b"password=", r.stdout)

    # -- fail loud when the deployment is broken ------------------------

    def test_a_missing_token_file_fails_loud(self):
        """`scripts/instrument.py`'s `resolve_binary()` lesson: a
        credential path that silently resolves to nothing does not stop
        anything, it makes every later git command fail with an
        authentication error that reads like a permissions problem on the
        remote. The message names the PATH and never the contents.
        """
        missing = str(Path(self._tmp) / "not-there.token")
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": missing}):
            with self.assertRaises(HubError) as ctx:
                credential_env()
        self.assertIn(missing, str(ctx.exception))

    def test_an_unreadable_token_file_fails_loud(self):
        if os.geteuid() == 0:
            self.skipTest("running as root: file mode 0 is still readable")
        locked = str(Path(self._tmp) / "locked.token")
        Path(locked).write_text(self.TOKEN + "\n")
        os.chmod(locked, 0o000)
        self.addCleanup(os.chmod, locked, 0o600)
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": locked}):
            with self.assertRaises(HubError):
                credential_env()

    def test_a_directory_instead_of_a_token_file_fails_loud(self):
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": self._tmp}):
            with self.assertRaises(HubError):
                credential_env()

    def test_constructing_a_hub_with_a_broken_token_path_fails_loud(self):
        """The failure has to surface at the FIRST git command, not at the
        first authenticated one -- otherwise a misconfigured host looks
        healthy right up until it tries to write a claim.
        """
        missing = str(Path(self._tmp) / "nope.token")
        with mock.patch.dict(os.environ, {"FLEET_GIT_TOKEN_FILE": missing}):
            with self.assertRaises(HubError):
                Hub(url=str(Path(self._tmp) / "hub.git"), workdir=str(Path(self._tmp) / "wd"))



if __name__ == "__main__":
    unittest.main()
