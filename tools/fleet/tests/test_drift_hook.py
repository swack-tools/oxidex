#!/usr/bin/env python3
"""Tests for the tip signal: `drift.bump_tip_signal` and the
`hooks/post-receive` script that calls it.

Everything here runs against a throwaway `git init --bare` repo created
under the system temp dir -- never the production hub
(`work2.oxidex.net`). `DriftHookTestCase.setUp` asserts this before any
test body runs, same guard as `tests/test_fleetlib.py`.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import drift  # noqa: E402
from _env import HermeticCase, scrub_env  # noqa: E402

FLEET_DIR = Path(__file__).resolve().parents[1]
HOOK_SCRIPT = FLEET_DIR / "hooks" / "post-receive"


def _run_git(args, cwd=None, input_bytes=None, env=None):
    full_env = scrub_env()
    full_env.update(
        {
            "GIT_AUTHOR_NAME": "t",
            "GIT_AUTHOR_EMAIL": "t@t",
            "GIT_COMMITTER_NAME": "t",
            "GIT_COMMITTER_EMAIL": "t@t",
            "GIT_TERMINAL_PROMPT": "0",
        }
    )
    if env:
        full_env.update(env)
    return subprocess.run(args, cwd=cwd, input=input_bytes, capture_output=True, env=full_env)


class DriftHookTestCase(HermeticCase):
    """Base fixture: a throwaway bare repo standing in for the hub."""

    def setUp(self):
        super().setUp()
        self._tmp_root = tempfile.mkdtemp(prefix="drift-hook-test-")
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.hub_path = str(Path(self._tmp_root) / "hub.git")

        init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        # Same non-negotiable guard as test_fleetlib.py: never let this
        # fixture -- or anything derived from it -- resolve to anything
        # but a temp path.
        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

    def _install_hook(self):
        """Copy hooks/post-receive + drift.py + fleetlib.py + config.py
        (fleetlib's sibling import) into the fixture hub's hooks/ dir,
        exactly as the T1.3 install instructions describe -- but ONLY
        into this throwaway repo, never the real hub.
        """
        hooks_dir = Path(self.hub_path) / "hooks"
        shutil.copy2(HOOK_SCRIPT, hooks_dir / "post-receive")
        shutil.copy2(FLEET_DIR / "drift.py", hooks_dir / "drift.py")
        shutil.copy2(FLEET_DIR / "fleetlib.py", hooks_dir / "fleetlib.py")
        shutil.copy2(FLEET_DIR / "config.py", hooks_dir / "config.py")
        target = hooks_dir / "post-receive"
        target.chmod(target.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)

    def _make_source_clone(self):
        src = tempfile.mkdtemp(prefix="drift-hook-src-")
        self.addCleanup(shutil.rmtree, src, ignore_errors=True)
        clone = _run_git(["git", "clone", "--quiet", self.hub_path, src])
        self.assertEqual(clone.returncode, 0, msg=clone.stderr.decode())
        return src

    def _read_signal(self):
        """Read refs/fleet/signals/tip directly via git plumbing (not
        fleetlib -- keeps this test independent of T0.2's module so a
        regression in one doesn't mask a regression in the other).
        """
        rev = _run_git(["git", "--git-dir", self.hub_path, "rev-parse", "--verify", "--quiet",
                         "refs/fleet/signals/tip"])
        if rev.returncode != 0:
            return None
        commit_sha = rev.stdout.decode().strip()
        cat = _run_git(["git", "--git-dir", self.hub_path, "cat-file", "-p",
                         f"{commit_sha}:payload.json"])
        self.assertEqual(cat.returncode, 0, msg=cat.stderr.decode())
        import json
        return json.loads(cat.stdout.decode())


class TestBumpTipSignalDirect(DriftHookTestCase):
    """Exercises drift.bump_tip_signal() directly against the bare repo,
    with no hook or push involved -- the unit closest to the CAS loop
    itself.
    """

    def test_first_bump_creates_generation_one(self):
        sha = "a" * 40
        payload = drift.bump_tip_signal(self.hub_path, sha)
        self.assertEqual(payload["generation"], 1)
        self.assertEqual(payload["sha"], sha)

        stored = self._read_signal()
        self.assertEqual(stored["generation"], 1)
        self.assertEqual(stored["sha"], sha)

    def test_sequential_bumps_increment_generation(self):
        first = drift.bump_tip_signal(self.hub_path, "a" * 40)
        second = drift.bump_tip_signal(self.hub_path, "b" * 40)
        third = drift.bump_tip_signal(self.hub_path, "c" * 40)
        self.assertEqual([first["generation"], second["generation"], third["generation"]], [1, 2, 3])

        stored = self._read_signal()
        self.assertEqual(stored["generation"], 3)
        self.assertEqual(stored["sha"], "c" * 40)

    def test_stale_cas_write_cannot_land(self):
        """Direct evidence, bypassing bump_tip_signal's retry loop, that a
        `git update-ref <ref> <new> <stale-old>` is rejected outright --
        the primitive the retry loop depends on to never lose or
        regress a generation.
        """
        drift.bump_tip_signal(self.hub_path, "a" * 40)
        current = _run_git(["git", "--git-dir", self.hub_path, "rev-parse", "refs/fleet/signals/tip"])
        current_sha = current.stdout.decode().strip()

        stale_payload = drift._write_payload_commit(self.hub_path, {
            "schema_version": 1, "written_by": "t", "written_at": "x",
            "sha": "b" * 40, "generation": 99, "ts": "x",
        })
        bogus_old = "0" * 40
        result = _run_git(["git", "--git-dir", self.hub_path, "update-ref",
                            "refs/fleet/signals/tip", stale_payload, bogus_old])
        self.assertNotEqual(result.returncode, 0)

        unchanged = _run_git(["git", "--git-dir", self.hub_path, "rev-parse", "refs/fleet/signals/tip"])
        self.assertEqual(unchanged.stdout.decode().strip(), current_sha,
                          "a rejected CAS write must leave the ref untouched")


def _attempt_bump(hub_path: str, sha_suffix: str):
    """Top-level (picklable) worker for the multiprocessing race test.
    Each worker is its own OS process racing the same bare repo's
    refs/fleet/signals/tip -- real concurrency, not a mock, mirroring
    fleetlib's own TestConcurrentCreate.
    """
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    import drift as _drift  # re-import in the child process
    sha = (sha_suffix * 40)[:40]
    payload = _drift.bump_tip_signal(hub_path, sha)
    return payload["generation"], sha


class TestConcurrentBumps(DriftHookTestCase):
    def test_generation_is_monotonic_under_real_concurrent_pushes(self):
        """N real OS processes hammer bump_tip_signal against the same
        bare repo concurrently. The requirement (FLEET_PLAN.md T1.3):
        'two pushes racing must not lose a generation bump or go
        backwards.' Verified by checking the set of generations handed
        out is exactly {1..N} -- no duplicates (no lost update: two
        racers both winning the same generation) and no gaps (no
        skipped/overwritten generation).
        """
        n = 16
        digits = "0123456789abcdef"
        with ProcessPoolExecutor(max_workers=n) as pool:
            futures = [pool.submit(_attempt_bump, self.hub_path, digits[i]) for i in range(n)]
            results = [f.result() for f in as_completed(futures)]

        generations = sorted(gen for gen, _sha in results)
        self.assertEqual(
            generations, list(range(1, n + 1)),
            f"expected exactly the generations 1..{n} with no gaps or duplicates, got {generations}",
        )

        final = self._read_signal()
        self.assertEqual(final["generation"], n)
        # The final stored sha must be whichever racer wrote the highest
        # generation -- i.e. the payload and the generation counter were
        # updated atomically together, never independently.
        winner_sha = next(sha for gen, sha in results if gen == n)
        self.assertEqual(final["sha"], winner_sha)


class TestHookEndToEnd(DriftHookTestCase):
    """Wires the real `hooks/post-receive` script into a throwaway bare
    repo and pushes to it for real -- confirms the hook is actually
    triggered by a `refs/heads/refactor/tag-machinery` update, ignores
    other refs, and that the generation counter survives two sequential
    real pushes.
    """

    def test_push_to_tip_branch_bumps_signal(self):
        self._install_hook()
        src = self._make_source_clone()

        (Path(src) / "README").write_text("v1\n")
        _run_git(["git", "add", "README"], cwd=src)
        _run_git(["git", "commit", "--quiet", "-m", "v1"], cwd=src)
        _run_git(["git", "checkout", "--quiet", "-b", "refactor/tag-machinery"], cwd=src)
        push1 = _run_git(["git", "push", "--quiet", "origin", "refactor/tag-machinery"], cwd=src)
        self.assertEqual(push1.returncode, 0, msg=push1.stderr.decode())

        sha1 = _run_git(["git", "rev-parse", "HEAD"], cwd=src).stdout.decode().strip()
        signal1 = self._read_signal()
        self.assertIsNotNone(signal1, "post-receive did not create refs/fleet/signals/tip on first push")
        self.assertEqual(signal1["generation"], 1)
        self.assertEqual(signal1["sha"], sha1)

        (Path(src) / "README").write_text("v2\n")
        _run_git(["git", "commit", "--quiet", "-am", "v2"], cwd=src)
        push2 = _run_git(["git", "push", "--quiet", "origin", "refactor/tag-machinery"], cwd=src)
        self.assertEqual(push2.returncode, 0, msg=push2.stderr.decode())

        sha2 = _run_git(["git", "rev-parse", "HEAD"], cwd=src).stdout.decode().strip()
        signal2 = self._read_signal()
        self.assertEqual(signal2["generation"], 2)
        self.assertEqual(signal2["sha"], sha2)
        self.assertNotEqual(sha1, sha2)

    def test_push_to_unrelated_branch_does_not_bump_signal(self):
        self._install_hook()
        src = self._make_source_clone()

        (Path(src) / "README").write_text("v1\n")
        _run_git(["git", "add", "README"], cwd=src)
        _run_git(["git", "commit", "--quiet", "-m", "v1"], cwd=src)
        _run_git(["git", "checkout", "--quiet", "-b", "staging/not-the-tip"], cwd=src)
        push = _run_git(["git", "push", "--quiet", "origin", "staging/not-the-tip"], cwd=src)
        self.assertEqual(push.returncode, 0, msg=push.stderr.decode())

        self.assertIsNone(self._read_signal(), "a push to a non-tip branch must not create the signal")


if __name__ == "__main__":
    unittest.main()
