#!/usr/bin/env python3
"""Tests for R1's tip-protection guard (T3, ARCH-FIX-SPEC.md):
`tools/fleet/hooks/pre-receive` (the actual enforcement point --
verifies the `train-token=<secret>` push option before a write to
`refs/heads/main` / `refs/heads/refactor/tag-machinery` is accepted) and
`tools/fleet/hooks/update` (a second, independent layer that denies
deletion of those two refs without needing push-option access -- see
that file's header for why it structurally cannot check the token).
Also exercises `tools/fleet/rollout/install_hook.sh`, which installs
both hooks (alongside the pre-existing post-receive) chained against any
prior hook of the same type, and the `push_options` plumbing added to
`tools/fleet/fleetlib.py`'s `Hub`.

WHY THIS FILE IS NAMED `test_update_hook.py` BUT MOSTLY EXERCISES
`hooks/pre-receive`: the T3 brief asked for a hub-side `update` hook
gated on a push option. Empirically (see TestPushOptionEnvVisibility
below, and the header comments on both hook files), git does not export
push-option env vars to the `update` hook -- only to `pre-receive` and
`post-receive` -- so the token check had to move to `pre-receive` to
work at all. This file keeps the name the brief specified while testing
the actual, working implementation.

Everything here runs against throwaway `git init --bare` repos created
under the system temp dir -- never the production hub
(`work2.oxidex.net`). Every TestCase's `setUp` asserts this, the same
guard `tests/test_fleetlib.py` and `tests/test_drift_hook.py` use.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from fleetlib import Hub, HubUnreachableError  # noqa: E402

FLEET_DIR = Path(__file__).resolve().parents[1]
INSTALL_SCRIPT = FLEET_DIR / "rollout" / "install_hook.sh"
PRE_RECEIVE_SCRIPT = FLEET_DIR / "hooks" / "pre-receive"
UPDATE_SCRIPT = FLEET_DIR / "hooks" / "update"

TIP_REF = "refs/heads/refactor/tag-machinery"
MAIN_REF = "refs/heads/main"
ZERO_SHA = "0" * 40


def _run_git(args, cwd=None, input_bytes=None, env=None):
    import os

    full_env = dict(os.environ)
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


class UpdateHookTestCase(unittest.TestCase):
    """Base fixture: a throwaway bare repo standing in for the hub, plus
    helpers to install the real hooks via the real installer and drive
    pushes against it exactly as a real client would.
    """

    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="update-hook-test-")
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        self.workdir = str(Path(self._tmp_root) / "cache")

        init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        # Same non-negotiable guard as test_fleetlib.py / test_drift_hook.py:
        # never let this fixture -- or anything derived from it -- resolve
        # to anything but a temp path.
        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

    def _install(self):
        """Install via the REAL installer script (--execute), against this
        throwaway hub only -- exercises install_hook.sh itself, not just
        the hook files it copies.
        """
        result = subprocess.run(
            ["bash", str(INSTALL_SCRIPT), self.hub_path, "--execute"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        return result

    def _token(self) -> str:
        return (Path(self.hub_path) / "train.token").read_text().strip()

    def _clone(self):
        src = tempfile.mkdtemp(prefix="update-hook-src-")
        self.addCleanup(shutil.rmtree, src, ignore_errors=True)
        clone = _run_git(["git", "clone", "--quiet", self.hub_path, src])
        self.assertEqual(clone.returncode, 0, msg=clone.stderr.decode())
        _run_git(["git", "config", "user.email", "t@t"], cwd=src)
        _run_git(["git", "config", "user.name", "t"], cwd=src)
        return src

    def _commit(self, src, message="c") -> str:
        _run_git(["git", "commit", "--quiet", "--allow-empty", "-m", message], cwd=src)
        return _run_git(["git", "rev-parse", "HEAD"], cwd=src).stdout.decode().strip()

    def _push(self, src, refspec, push_option=None, force=False):
        args = ["git", "push"]
        if force:
            args += ["--force"]
        if push_option:
            args += ["-o", push_option]
        args += ["origin", refspec]
        return _run_git(args, cwd=src)

    def _reflog(self, ref):
        return _run_git(["git", "--git-dir", self.hub_path, "reflog", "show", ref])

    def _remote_sha(self, ref):
        result = _run_git(["git", "--git-dir", self.hub_path, "rev-parse", "--verify", "--quiet", ref])
        if result.returncode != 0:
            return None
        return result.stdout.decode().strip()


# --------------------------------------------------------------------- #
# Core deny/allow matrix (the scenarios T3's brief names explicitly).
# --------------------------------------------------------------------- #


class TestDenyMatrix(UpdateHookTestCase):
    def test_tokenless_push_to_tip_denied(self):
        self._install()
        src = self._clone()
        self._commit(src)
        result = self._push(src, f"HEAD:{TIP_REF}")
        self.assertNotEqual(result.returncode, 0)
        stderr = result.stderr.decode()
        self.assertIn("tip-guard: DENY ref=refs/heads/refactor/tag-machinery", stderr)
        self.assertIn("reason=missing or incorrect train-token", stderr)
        self.assertIn("remedy=", stderr)
        self.assertIsNone(self._remote_sha(TIP_REF), "a denied push must not land")

    def test_correct_token_push_to_tip_allowed(self):
        self._install()
        token = self._token()
        src = self._clone()
        sha = self._commit(src)
        result = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}")
        self.assertEqual(result.returncode, 0, msg=result.stderr.decode())
        self.assertEqual(self._remote_sha(TIP_REF), sha)

    def test_wrong_token_push_to_tip_denied(self):
        self._install()
        src = self._clone()
        self._commit(src)
        result = self._push(src, f"HEAD:{TIP_REF}", push_option="train-token=not-the-secret")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("tip-guard: DENY", result.stderr.decode())
        self.assertIsNone(self._remote_sha(TIP_REF))

    def test_tokenless_push_to_staging_allowed(self):
        self._install()
        src = self._clone()
        sha = self._commit(src)
        result = self._push(src, "HEAD:refs/heads/staging/x")
        self.assertEqual(result.returncode, 0, msg=result.stderr.decode())
        self.assertEqual(self._remote_sha("refs/heads/staging/x"), sha)

    def test_tip_deletion_with_valid_token_denied(self):
        self._install()
        token = self._token()
        src = self._clone()
        sha = self._commit(src)
        landed = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}")
        self.assertEqual(landed.returncode, 0, msg=landed.stderr.decode())

        deletion = self._push(src, f":{TIP_REF}", push_option=f"train-token={token}")
        self.assertNotEqual(deletion.returncode, 0, "deletion must be denied EVEN WITH a correct token")
        stderr = deletion.stderr.decode()
        self.assertIn("tip-guard: DENY ref=refs/heads/refactor/tag-machinery", stderr)
        self.assertIn("reason=deletion of a protected ref is never permitted", stderr)
        self.assertEqual(self._remote_sha(TIP_REF), sha, "tip must still be present after the denied deletion")

    def test_forced_non_fast_forward_push_to_tip_with_valid_token_denied(self):
        """R1's history-rewrite sub-clause, closed by install_hook.sh's
        `receive.denyNonFastForwards=true`. A VALID token is enough to
        land an ordinary fast-forward, but not enough to force-push a
        REWRITE of history the tip already advanced past -- the same
        tip-integrity failure `test_tip_deletion_with_valid_token_denied`
        proves for outright deletion, here via a non-fast-forward update
        instead. This is a git-transport-level guard, unconditional and
        independent of the token check, so it must deny this push even
        though its `-o train-token=...` is exactly what a legitimate train
        push carries.
        """
        self._install()
        token = self._token()
        src = self._clone()
        landed_sha = self._commit(src, message="first")
        landed = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}")
        self.assertEqual(landed.returncode, 0, msg=landed.stderr.decode())
        self.assertEqual(self._remote_sha(TIP_REF), landed_sha)

        # Rewrite history in place: amend the just-landed commit into a
        # SIBLING, not a descendant, of what the hub already has -- a
        # genuine non-fast-forward, not merely a second commit stacked on
        # top (which a fast-forward push would land just fine and this
        # test would then prove nothing).
        amend = _run_git(
            ["git", "commit", "--amend", "--allow-empty", "--quiet", "-m", "rewritten"], cwd=src,
        )
        self.assertEqual(amend.returncode, 0, msg=amend.stderr.decode())
        rewritten_sha = _run_git(["git", "rev-parse", "HEAD"], cwd=src).stdout.decode().strip()
        self.assertNotEqual(rewritten_sha, landed_sha, "sanity: amend must produce a different sha")

        forced = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}", force=True)
        self.assertNotEqual(
            forced.returncode, 0,
            "a force-push carrying a VALID token must still be denied -- "
            "receive.denyNonFastForwards, not the token check, is what stops this",
        )
        self.assertEqual(
            self._remote_sha(TIP_REF), landed_sha,
            "the tip must be UNCHANGED after the denied force-push, not silently rewritten",
        )

    def test_main_ref_protected_the_same_as_tip(self):
        """`refs/heads/main` does not exist on the hub yet in this fleet
        (only refactor/tag-machinery does today), but R1 names it
        explicitly as a second protected ref -- verify it is guarded
        identically, including for its very first (create) push.
        """
        self._install()
        src = self._clone()
        self._commit(src)
        denied = self._push(src, f"HEAD:{MAIN_REF}")
        self.assertNotEqual(denied.returncode, 0)
        self.assertIn("tip-guard: DENY ref=refs/heads/main", denied.stderr.decode())
        self.assertIsNone(self._remote_sha(MAIN_REF))

        token = self._token()
        allowed = self._push(src, f"HEAD:{MAIN_REF}", push_option=f"train-token={token}")
        self.assertEqual(allowed.returncode, 0, msg=allowed.stderr.decode())
        self.assertIsNotNone(self._remote_sha(MAIN_REF))

    def test_missing_token_file_fails_closed(self):
        """Per R1: a missing train.token on the hub must fail CLOSED for
        the two protected refs, not open.
        """
        self._install()
        Path(self.hub_path, "train.token").unlink()
        src = self._clone()
        self._commit(src)
        result = self._push(src, f"HEAD:{TIP_REF}", push_option="train-token=anything")
        self.assertNotEqual(result.returncode, 0)
        stderr = result.stderr.decode()
        self.assertIn("tip-guard: DENY", stderr)
        self.assertIn("no train.token on the hub", stderr)

    def test_staging_ref_deletion_is_unaffected(self):
        """Only the two protected refs get unconditional deletion-denial;
        an ordinary staging ref delete (routine train cleanup) must keep
        working exactly as before this hook existed.
        """
        self._install()
        src = self._clone()
        self._commit(src)
        create = self._push(src, "HEAD:refs/heads/staging/y")
        self.assertEqual(create.returncode, 0, msg=create.stderr.decode())
        delete = self._push(src, ":refs/heads/staging/y")
        self.assertEqual(delete.returncode, 0, msg=delete.stderr.decode())
        self.assertIsNone(self._remote_sha("refs/heads/staging/y"))


# --------------------------------------------------------------------- #
# Reflog observability + chain integrity with the pre-existing
# post-receive hook.
# --------------------------------------------------------------------- #


class TestObservabilityAndChaining(UpdateHookTestCase):
    def test_reflog_entries_exist_after_allowed_update(self):
        self._install()
        token = self._token()
        src = self._clone()
        sha1 = self._commit(src, "c1")
        r1 = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}")
        self.assertEqual(r1.returncode, 0, msg=r1.stderr.decode())

        (Path(src) / "f").write_text("v2\n")
        _run_git(["git", "add", "f"], cwd=src)
        sha2 = self._commit(src, "c2")
        r2 = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}")
        self.assertEqual(r2.returncode, 0, msg=r2.stderr.decode())

        reflog = self._reflog(TIP_REF)
        self.assertEqual(reflog.returncode, 0, msg=reflog.stderr.decode())
        lines = reflog.stdout.decode().strip().splitlines()
        self.assertGreaterEqual(
            len(lines), 2,
            f"expected at least 2 reflog entries (one per allowed push) via "
            f"core.logAllRefUpdates, got: {lines!r}",
        )
        self.assertIn(sha2[:7], lines[0])

    def test_post_receive_still_fires_after_an_allowed_update(self):
        """Chain integrity: installing pre-receive/update must not disturb
        the pre-existing post-receive tip-signal hook (T1.3) -- they are
        different hook types and must never conflict.
        """
        self._install()
        token = self._token()
        src = self._clone()
        sha = self._commit(src)
        result = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}")
        self.assertEqual(result.returncode, 0, msg=result.stderr.decode())

        signal_sha = self._remote_sha("refs/fleet/signals/tip")
        self.assertIsNotNone(
            signal_sha,
            "post-receive's refs/fleet/signals/tip bump did not fire after an allowed tip update",
        )
        cat = _run_git(["git", "--git-dir", self.hub_path, "cat-file", "-p", f"{signal_sha}:payload.json"])
        self.assertEqual(cat.returncode, 0, msg=cat.stderr.decode())
        import json

        payload = json.loads(cat.stdout.decode())
        self.assertEqual(payload["sha"], sha)
        self.assertEqual(payload["generation"], 1)


# --------------------------------------------------------------------- #
# The `update` hook as an independent, argv-only layer -- exercised
# directly (no push, no push options in play at all) to prove it does not
# depend on the token check to deny a protected-ref deletion.
# --------------------------------------------------------------------- #


class TestUpdateHookDirect(unittest.TestCase):
    def test_denies_deletion_of_protected_ref_via_argv_alone(self):
        result = subprocess.run(
            ["bash", str(UPDATE_SCRIPT), TIP_REF, "a" * 40, ZERO_SHA],
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("tip-guard: DENY ref=refs/heads/refactor/tag-machinery", result.stderr)
        self.assertIn("reason=deletion of a protected ref is never permitted", result.stderr)

    def test_denies_deletion_of_main_via_argv_alone(self):
        result = subprocess.run(
            ["bash", str(UPDATE_SCRIPT), MAIN_REF, "a" * 40, ZERO_SHA],
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)

    def test_allows_deletion_of_unprotected_ref(self):
        result = subprocess.run(
            ["bash", str(UPDATE_SCRIPT), "refs/heads/staging/z", "a" * 40, ZERO_SHA],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)

    def test_allows_creation_of_protected_ref(self):
        """The update hook cannot see push options, so it must not attempt
        to gate anything but deletion -- creation/update of a protected
        ref is pre-receive's job entirely.
        """
        result = subprocess.run(
            ["bash", str(UPDATE_SCRIPT), TIP_REF, ZERO_SHA, "b" * 40],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)


# --------------------------------------------------------------------- #
# Installer chaining: a pre-existing pre-receive/update hook is preserved
# and both halves' rejections propagate.
# --------------------------------------------------------------------- #


class TestInstallerChaining(UpdateHookTestCase):
    def _write_legacy_hook(self, name: str, marker: str):
        hooks_dir = Path(self.hub_path) / "hooks"
        path = hooks_dir / name
        path.write_text(f'#!/bin/bash\necho "{marker}" >&2\nexit 0\n')
        path.chmod(path.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)

    def test_legacy_pre_receive_is_preserved_and_still_runs(self):
        self._write_legacy_hook("pre-receive", "legacy-pre-receive-ran")
        self._install()
        self.assertTrue((Path(self.hub_path) / "hooks" / "pre-receive.legacy").is_file())

        src = self._clone()
        self._commit(src)
        result = self._push(src, "HEAD:refs/heads/staging/w")
        self.assertEqual(result.returncode, 0, msg=result.stderr.decode())
        self.assertIn("legacy-pre-receive-ran", result.stderr.decode())

    def test_legacy_pre_receive_allow_plus_fleet_deny_still_denies_overall(self):
        self._write_legacy_hook("pre-receive", "legacy-pre-receive-ran")
        self._install()
        src = self._clone()
        self._commit(src)
        result = self._push(src, f"HEAD:{TIP_REF}")
        self.assertNotEqual(result.returncode, 0, "legacy hook allowing must not override the fleet guard's deny")
        stderr = result.stderr.decode()
        self.assertIn("legacy-pre-receive-ran", stderr)
        self.assertIn("tip-guard: DENY", stderr)

    def test_legacy_update_is_preserved_and_still_runs(self):
        self._write_legacy_hook("update", "legacy-update-ran")
        self._install()
        self.assertTrue((Path(self.hub_path) / "hooks" / "update.legacy").is_file())

        token = self._token()
        src = self._clone()
        self._commit(src)
        result = self._push(src, f"HEAD:{TIP_REF}", push_option=f"train-token={token}")
        self.assertEqual(result.returncode, 0, msg=result.stderr.decode())
        self.assertIn("legacy-update-ran", result.stderr.decode())

    def test_reinstall_is_idempotent_and_preserves_token(self):
        self._install()
        token_before = self._token()
        self._install()
        token_after = self._token()
        self.assertEqual(token_before, token_after, "re-running the installer must never rotate an existing token")


class TestInstallerDryRun(UpdateHookTestCase):
    def test_dry_run_touches_nothing(self):
        result = subprocess.run(
            ["bash", str(INSTALL_SCRIPT), self.hub_path],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        hooks_dir = Path(self.hub_path) / "hooks"
        installed = {p.name for p in hooks_dir.iterdir() if not p.name.endswith(".sample")}
        self.assertEqual(installed, set(), f"dry-run must not write anything, found: {installed}")
        self.assertFalse((Path(self.hub_path) / "train.token").exists())
        cfg = _run_git(["git", "--git-dir", self.hub_path, "config", "--get", "receive.advertisePushOptions"])
        self.assertNotEqual(cfg.returncode, 0, "dry-run must not set repo config either")


# --------------------------------------------------------------------- #
# The empirical finding itself, pinned as a regression guard: push
# options reach pre-receive/post-receive but never the update hook.
# --------------------------------------------------------------------- #


class TestPushOptionEnvVisibility(unittest.TestCase):
    """If a future git version ever changes this, R1's design assumption
    breaks silently unless something asserts it. This test is that
    assertion -- verified empirically against git 2.54.0 (Apple Git-157)
    when written; see tools/fleet/hooks/pre-receive and .../update for the
    full write-up.
    """

    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="pushopt-env-test-")
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.hub_path = str(Path(self._tmp_root) / "hub.git")

        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(resolved.startswith(system_tmp))
        self.assertNotIn("work2.oxidex.net", resolved)

        init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())
        cfg = _run_git(["git", "--git-dir", self.hub_path, "config", "receive.advertisePushOptions", "true"])
        self.assertEqual(cfg.returncode, 0, msg=cfg.stderr.decode())

        hooks_dir = Path(self.hub_path) / "hooks"
        scripts = {
            "pre-receive": (
                "#!/usr/bin/env bash\n"
                "cat >/dev/null\n"
                f'echo "count=${{GIT_PUSH_OPTION_COUNT:-UNSET}}" > {self._tmp_root}/pre-receive.seen\n'
                "exit 0\n"
            ),
            "post-receive": (
                "#!/usr/bin/env bash\n"
                "cat >/dev/null\n"
                f'echo "count=${{GIT_PUSH_OPTION_COUNT:-UNSET}}" > {self._tmp_root}/post-receive.seen\n'
                "exit 0\n"
            ),
            "update": (
                "#!/usr/bin/env bash\n"
                f'echo "count=${{GIT_PUSH_OPTION_COUNT:-UNSET}}" > {self._tmp_root}/update.seen\n'
                "exit 0\n"
            ),
        }
        for name, content in scripts.items():
            path = hooks_dir / name
            path.write_text(content)
            path.chmod(path.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)

    def test_push_option_env_reaches_pre_and_post_receive_but_not_update(self):
        src = tempfile.mkdtemp(prefix="pushopt-env-src-")
        self.addCleanup(shutil.rmtree, src, ignore_errors=True)
        clone = _run_git(["git", "clone", "--quiet", self.hub_path, src])
        self.assertEqual(clone.returncode, 0, msg=clone.stderr.decode())
        _run_git(["git", "config", "user.email", "t@t"], cwd=src)
        _run_git(["git", "config", "user.name", "t"], cwd=src)
        _run_git(["git", "commit", "--quiet", "--allow-empty", "-m", "c"], cwd=src)

        push = _run_git(["git", "push", "-o", "probe=1", "origin", "HEAD:refs/heads/anyref"], cwd=src)
        self.assertEqual(push.returncode, 0, msg=push.stderr.decode())

        pre = (Path(self._tmp_root) / "pre-receive.seen").read_text().strip()
        post = (Path(self._tmp_root) / "post-receive.seen").read_text().strip()
        upd = (Path(self._tmp_root) / "update.seen").read_text().strip()

        self.assertEqual(pre, "count=1", "pre-receive should see the push option")
        self.assertEqual(post, "count=1", "post-receive should see the push option")
        self.assertEqual(
            upd, "count=UNSET",
            "update hook is NOT given push-option env vars by git -- this is the "
            "empirical finding that makes pre-receive, not update, the enforcement "
            "point for R1's token check",
        )


# --------------------------------------------------------------------- #
# fleetlib.py's push_options plumbing (R1 plumbing item 3): proves the
# kwarg reaches the underlying `git push -o ...` invocation and that
# every pre-existing call site (which omits it) is unaffected.
# --------------------------------------------------------------------- #


class TestFleetlibPushOptionsPlumbing(UpdateHookTestCase):
    def test_create_without_push_options_is_unchanged(self):
        # No receive.advertisePushOptions on this bare `git init --bare`
        # fixture at all -- create() must succeed exactly as it did before
        # this parameter existed, proving the omitted-default case is a
        # true no-op.
        hub = Hub(url=self.hub_path, workdir=self.workdir)
        self.assertTrue(hub.create("refs/fleet/test/plain", {"v": 1}))

    def test_create_with_push_options_reaches_the_git_subprocess(self):
        # Same fixture hub, still no receive.advertisePushOptions set.
        # Passing push_options must now make the underlying
        # `git push -o ...` get rejected at the transport level ("does not
        # support push options") -- not one of fleetlib's recognized
        # content-rejection patterns, so it surfaces as
        # HubUnreachableError. That is only possible if `-o` genuinely
        # reached the subprocess -- direct proof the plumbing threads
        # through, not just that the method accepts the kwarg.
        hub = Hub(url=self.hub_path, workdir=self.workdir)
        with self.assertRaises(HubUnreachableError):
            hub.create("refs/fleet/test/opt", {"v": 1}, push_options=["train-token=x"])

    def test_update_and_delete_also_thread_push_options(self):
        hub = Hub(url=self.hub_path, workdir=self.workdir)
        self.assertTrue(hub.create("refs/fleet/test/upd", {"v": 1}))
        sha = hub.sha("refs/fleet/test/upd")
        with self.assertRaises(HubUnreachableError):
            hub.update("refs/fleet/test/upd", {"v": 2}, expect_sha=sha, push_options=["x=y"])
        with self.assertRaises(HubUnreachableError):
            hub.delete("refs/fleet/test/upd", expect_sha=sha, push_options=["x=y"])
        # Unaffected without the kwarg:
        self.assertTrue(hub.update("refs/fleet/test/upd", {"v": 2}, expect_sha=sha))

    def _fetch_into_hub_cache(self, hub: Hub, src: str, sha: str):
        """`Hub.push_ref` pushes FROM the Hub's own local cache repo
        (`hub.workdir`), not from `src` -- so a commit only known to `src`
        (an unrelated clone this test made for convenience) must be
        fetched into the cache first, exactly as a real caller building a
        commit with `_write_commit` already has it locally. This mirrors
        real usage; it is not a workaround for a bug in `push_ref`.
        """
        fetch = _run_git(["git", "--git-dir", str(hub.workdir), "fetch", "--quiet", src, sha])
        self.assertEqual(fetch.returncode, 0, msg=fetch.stderr.decode())

    def test_push_ref_with_correct_token_lands_on_a_protected_ref(self):
        self._install()
        token = self._token()
        src = self._clone()
        sha = self._commit(src)
        hub = Hub(url=self.hub_path, workdir=self.workdir)
        self._fetch_into_hub_cache(hub, src, sha)
        result = hub.push_ref(f"{sha}:{TIP_REF}", push_options=[f"train-token={token}"])
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        self.assertEqual(self._remote_sha(TIP_REF), sha)

    def test_push_ref_without_token_is_denied_on_a_protected_ref(self):
        self._install()
        src = self._clone()
        sha = self._commit(src)
        hub = Hub(url=self.hub_path, workdir=self.workdir)
        self._fetch_into_hub_cache(hub, src, sha)
        result = hub.push_ref(f"{sha}:{TIP_REF}")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("tip-guard", result.stderr)
        self.assertIsNone(self._remote_sha(TIP_REF))


if __name__ == "__main__":
    unittest.main()
