#!/usr/bin/env python3
"""Tests for tools/fleet/fleetlib.py.

Everything here runs against a throwaway `git init --bare` repo created in
`setUp` -- never the production hub. `FleetlibTestCase.setUp` asserts the
bare repo's path lives under the system temp directory before any test
body runs, as a hard guard against ever accidentally pointing at
`work2.oxidex.net`.

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
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402


def _run_git(args, cwd=None, input_bytes=None):
    return subprocess.run(args, cwd=cwd, input=input_bytes, capture_output=True)


class FleetlibTestCase(unittest.TestCase):
    """Base fixture: a throwaway bare repo standing in for the hub."""

    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="fleetlib-test-")
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        self.workdir = str(Path(self._tmp_root) / "cache")

        init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        # The single most important assertion in this file: never let the
        # fixture -- or by extension any test -- point at anything but a
        # temp path. This is the guard against ever contacting the
        # production hub (work2.oxidex.net) from a test run.
        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

        self.hub = Hub(url=self.hub_path, workdir=self.workdir)

    def tearDown(self):
        shutil.rmtree(self._tmp_root, ignore_errors=True)

    def fresh_hub(self) -> Hub:
        """A second Hub instance with its own local cache, same remote --
        simulates a second host talking to the same hub.
        """
        other_workdir = tempfile.mkdtemp(prefix="fleetlib-test-cache2-")
        self.addCleanup(shutil.rmtree, other_workdir, ignore_errors=True)
        return Hub(url=self.hub_path, workdir=other_workdir)


class TestFixtureGuard(FleetlibTestCase):
    def test_hub_url_is_a_temp_path(self):
        self.assertTrue(self.hub.url.startswith(tempfile.gettempdir()) or "/T/" in self.hub.url or True)
        resolved = str(Path(self.hub.url).resolve())
        self.assertTrue(resolved.startswith(str(Path(tempfile.gettempdir()).resolve())))


class TestCreate(FleetlibTestCase):
    def test_create_then_read(self):
        ok = self.hub.create("refs/fleet/test/one", {"work_key": "abc"})
        self.assertTrue(ok)
        payload = self.hub.read("refs/fleet/test/one")
        self.assertIsNotNone(payload)
        self.assertEqual(payload["work_key"], "abc")
        self.assertEqual(payload["schema_version"], 1)
        self.assertIn("written_by", payload)
        self.assertIn("written_at", payload)

    def test_create_twice_second_fails_first_payload_intact(self):
        first = self.hub.create("refs/fleet/test/dup", {"holder_host": "hostA"})
        self.assertTrue(first)

        second = self.hub.create("refs/fleet/test/dup", {"holder_host": "hostB"})
        self.assertFalse(second)

        payload = self.hub.read("refs/fleet/test/dup")
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


def _attempt_create(hub_path: str, ref: str, idx: int):
    """Top-level (picklable) worker for the multiprocessing race test.
    Each worker builds its own Hub with its own local cache dir, so the
    only shared state is the remote bare repo -- real inter-process
    contention on the actual CAS primitive, not a mock.
    """
    workdir = tempfile.mkdtemp(prefix=f"fleetlib-race-{idx}-")
    try:
        hub = Hub(url=hub_path, workdir=workdir)
        ok = hub.create(ref, {"claimant_idx": idx})
        return idx, ok
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


class TestConcurrentCreate(FleetlibTestCase):
    def test_concurrent_create_exactly_one_winner(self):
        ref = "refs/fleet/test/race"
        n = 12

        with ProcessPoolExecutor(max_workers=n) as pool:
            futures = [pool.submit(_attempt_create, self.hub_path, ref, i) for i in range(n)]
            results = [f.result() for f in as_completed(futures)]

        winners = [idx for idx, ok in results if ok]
        self.assertEqual(
            len(winners),
            1,
            msg=f"expected exactly one winner among {n} real OS-process racers, got {winners}",
        )

        payload = self.hub.read(ref)
        self.assertIsNotNone(payload)
        self.assertEqual(payload["claimant_idx"], winners[0])


class TestUpdate(FleetlibTestCase):
    def test_update_with_correct_expect_sha_succeeds(self):
        self.hub.create("refs/fleet/test/upd", {"v": 1})
        cur_sha = self.hub.sha("refs/fleet/test/upd")
        ok = self.hub.update("refs/fleet/test/upd", {"v": 2}, expect_sha=cur_sha)
        self.assertTrue(ok)
        self.assertEqual(self.hub.read("refs/fleet/test/upd")["v"], 2)

    def test_update_with_stale_expect_sha_fails_ref_unchanged(self):
        self.hub.create("refs/fleet/test/upd2", {"v": 1})
        stale_sha = self.hub.sha("refs/fleet/test/upd2")

        # A legitimate update moves the ref forward...
        ok1 = self.hub.update("refs/fleet/test/upd2", {"v": 2}, expect_sha=stale_sha)
        self.assertTrue(ok1)
        current_sha_after = self.hub.sha("refs/fleet/test/upd2")

        # ...so a second writer still holding the old sha must lose.
        ok2 = self.hub.update("refs/fleet/test/upd2", {"v": 99}, expect_sha=stale_sha)
        self.assertFalse(ok2)

        self.assertEqual(self.hub.sha("refs/fleet/test/upd2"), current_sha_after)
        self.assertEqual(self.hub.read("refs/fleet/test/upd2")["v"], 2)


class TestDelete(FleetlibTestCase):
    def test_delete_with_correct_expect_sha_succeeds(self):
        self.hub.create("refs/fleet/test/del", {"v": 1})
        cur_sha = self.hub.sha("refs/fleet/test/del")
        ok = self.hub.delete("refs/fleet/test/del", expect_sha=cur_sha)
        self.assertTrue(ok)
        self.assertIsNone(self.hub.read("refs/fleet/test/del"))
        self.assertIsNone(self.hub.sha("refs/fleet/test/del"))

    def test_delete_with_stale_expect_sha_fails_ref_unchanged(self):
        self.hub.create("refs/fleet/test/del2", {"v": 1})
        stale_sha = self.hub.sha("refs/fleet/test/del2")

        self.hub.update("refs/fleet/test/del2", {"v": 2}, expect_sha=stale_sha)
        current_sha = self.hub.sha("refs/fleet/test/del2")

        ok = self.hub.delete("refs/fleet/test/del2", expect_sha=stale_sha)
        self.assertFalse(ok)
        self.assertEqual(self.hub.sha("refs/fleet/test/del2"), current_sha)
        self.assertIsNotNone(self.hub.read("refs/fleet/test/del2"))

    def test_raw_git_force_with_lease_on_delete(self):
        """Direct evidence that `--force-with-lease=<ref>:<sha>` actually
        guards a *deletion*, not just an update, for this git version. The
        spec explicitly asks this not be assumed.
        """
        version = subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip()

        self.hub.create("refs/fleet/test/rawdel", {"v": 1})
        real_sha = self.hub.sha("refs/fleet/test/rawdel")
        wrong_sha = "0" * 40

        # Stale-lease delete must be rejected, not silently accepted as an
        # unconditional force.
        r_bad = _run_git(
            [
                "git",
                "--git-dir",
                self.workdir,
                "push",
                f"--force-with-lease=refs/fleet/test/rawdel:{wrong_sha}",
                self.hub_path,
                ":refs/fleet/test/rawdel",
            ]
        )
        self.assertNotEqual(
            r_bad.returncode,
            0,
            msg=f"[{version}] --force-with-lease delete with a wrong expected sha unexpectedly succeeded",
        )
        self.assertIsNotNone(self.hub.sha("refs/fleet/test/rawdel"))

        # Correct-lease delete must succeed.
        r_good = _run_git(
            [
                "git",
                "--git-dir",
                self.workdir,
                "push",
                f"--force-with-lease=refs/fleet/test/rawdel:{real_sha}",
                self.hub_path,
                ":refs/fleet/test/rawdel",
            ]
        )
        self.assertEqual(
            r_good.returncode,
            0,
            msg=f"[{version}] --force-with-lease delete with the correct expected sha failed: "
            f"{r_good.stderr.decode()}",
        )
        self.assertIsNone(self.hub.sha("refs/fleet/test/rawdel"))


class TestRead(FleetlibTestCase):
    def test_read_of_absent_ref_is_none_not_exception(self):
        self.assertIsNone(self.hub.read("refs/fleet/test/does-not-exist"))
        self.assertIsNone(self.hub.sha("refs/fleet/test/does-not-exist"))

    def test_payload_roundtrips_unicode_and_nested_dicts(self):
        payload = {
            "title": "Route legacy formats — SWF, PICT, PPM, RA, 京セラ RAW 🎯",
            "scope": {
                "formats": ["SWF", "PICT", "PPM", "RA", "KyoceraRAW"],
                "nested": {"deeper": {"deepest": ["a", 1, None, True, 3.14]}},
            },
            "note": "quotes \" and backslashes \\ and newlines\nshould survive",
        }
        self.hub.create("refs/fleet/test/unicode", payload)
        got = self.hub.read("refs/fleet/test/unicode")
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

    REF = "refs/fleet/test/racy"

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
        self.assertEqual((None, None), self.hub.read_with_sha("refs/fleet/test/nope"))
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus-coh-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.read_with_sha("refs/fleet/test/whatever")

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
        push = _run_git(git + ["push", self.hub_path, f"{commit_sha}:refs/fleet/test/empty"])
        self.assertEqual(push.returncode, 0, msg=push.stderr.decode())

        with self.assertRaises(HubError) as ctx:
            self.fresh_hub().read("refs/fleet/test/empty")
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

    REF = "refs/fleet/test/classify"

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
            "fatal: couldn't find remote ref refs/fleet/test/classify\n"
            "fatal: Could not read from remote repository.\n",
        )
        with self.assertRaises(HubUnreachableError):
            reader.read(self.REF)


class TestList(FleetlibTestCase):
    def test_list_returns_matching_refs_only(self):
        self.hub.create("refs/fleet/claims/gate/aaa", {"k": 1})
        self.hub.create("refs/fleet/claims/gate/bbb", {"k": 2})
        self.hub.create("refs/fleet/claims/agent/ccc", {"k": 3})
        self.hub.create("refs/fleet/verdicts/ddd", {"k": 4})

        gate_claims = self.hub.list("refs/fleet/claims/gate")
        self.assertEqual(set(gate_claims.keys()), {"refs/fleet/claims/gate/aaa", "refs/fleet/claims/gate/bbb"})

        all_claims = self.hub.list("refs/fleet/claims/")
        self.assertEqual(
            set(all_claims.keys()),
            {"refs/fleet/claims/gate/aaa", "refs/fleet/claims/gate/bbb", "refs/fleet/claims/agent/ccc"},
        )


class TestUnreachable(FleetlibTestCase):
    def test_unreachable_hub_raises_on_sha(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.sha("refs/fleet/test/whatever")

    def test_unreachable_hub_raises_on_read_not_none(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus2-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.read("refs/fleet/test/whatever")

    def test_unreachable_hub_raises_on_create(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus3-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.create("refs/fleet/test/whatever", {"x": 1})

    def test_unreachable_hub_raises_on_list(self):
        bogus_workdir = tempfile.mkdtemp(prefix="fleetlib-bogus4-")
        self.addCleanup(shutil.rmtree, bogus_workdir, ignore_errors=True)
        bogus = Hub(url="/definitely/does/not/exist/on/this/machine.git", workdir=bogus_workdir)
        with self.assertRaises(HubUnreachableError):
            bogus.list("refs/fleet/test/")


if __name__ == "__main__":
    unittest.main()
