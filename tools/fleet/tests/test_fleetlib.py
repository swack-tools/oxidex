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
import shutil
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from fleetlib import Hub, HubUnreachableError  # noqa: E402


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
