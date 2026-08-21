#!/usr/bin/env python3
"""LIVE test: the code repo's guard rulesets actually reject what they claim.

Opt-in. Does nothing unless `FLEET_LIVE_GITHUB=1`, because it performs real
pushes to the real public code repo. It is deliberately NOT under
`tools/fleet/tests/` directly: `gate.sh`'s `_fleet_test_modules()` globs
`tools/fleet/tests/test_*.py` (direct children only, gate.sh L207-215), so a
file one directory down can never be dragged into a gate run by accident.

    FLEET_LIVE_GITHUB=1 python3 tools/fleet/tests/tests_live_runner    # no
    FLEET_LIVE_GITHUB=1 python3 tools/fleet/tests/live/test_tip_ruleset.py
    ( cd tools/fleet/tests && FLEET_LIVE_GITHUB=1 python3 -m unittest live.test_tip_ruleset )

WHY `keel-proof/*` AND NOT THE TIP
----------------------------------
This replaces `test_update_hook.TestDenyMatrix` + `test_drift_hook`, which
could exercise the real protected refs because the hub was a throwaway bare
repo in `/tmp`. GitHub rulesets have no fixture: the only way to prove a
ruleset rejects a force-push is to attempt one against the live repo. So
SPEC §8 gives the ruleset pair its own target namespace,
`refs/heads/keel-proof/*`, carrying an identical `proof-guard`
(deletion + non_fast_forward) to `tip-guard`. Proving the rule text on
`keel-proof/x` proves it on the tip, because it is the same rule text --
`tools/fleet/rollout/rulesets.py` builds both from the one `GUARD_RULES`
list, and `test_rulesets.py::TestGuardParity` pins that.

WHAT THIS ASSERTS AT THE CURRENT (GUARD-ONLY) STAGE
---------------------------------------------------
`proof-update` (restrict-updates) is NOT yet applied -- it cannot be until
the `keel-train` deploy key exists (see `rulesets.py`'s TODO(keel-train)),
so `rulesets.py apply` skips it by default. With only `proof-guard` active
the matrix is:

  1. keyless fast-forward push  -> rc 0        (guards do not restrict updates)
  2. keyless force push         -> rc != 0, GH013 in stderr
  3. keyless delete             -> rc != 0
  4. the ref still points at (1)'s sha afterwards

"keyless" = the ordinary host credential (HTTPS + git credential helper),
i.e. NOT the train deploy key. Case 1 flipping to rc != 0 with GH013 is the
signal that `proof-update` has since been applied; the assertion message
says so rather than leaving a future reader to guess.

PLAN Stage 1's full acceptance matrix additionally covers the deploy-key
cases (deploy-key FF -> 0, deploy-key --force -> non-zero, deploy-key delete
-> non-zero, PAT push to `rescued/proof` then delete -> non-zero). Those need
the key; `TestDeployKeyMatrix` below is written and skips itself until
`FLEET_TRAIN_DEPLOY_KEY` points at one.

SIDE EFFECTS. Case 1 advances `refs/heads/keel-proof/x` by one empty commit
per run and cases 2-3 prove that ref cannot be cleaned up -- by design; that
is what `proof-guard` means. One ref, reused, so the namespace does not grow.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # tools/fleet

LIVE_ENV = "FLEET_LIVE_GITHUB"
CODE_URL = os.environ.get(
    "FLEET_LIVE_CODE_URL", "https://github.com/swack-tools/oxidex.git"
)
PROOF_REF = os.environ.get("FLEET_LIVE_PROOF_REF", "refs/heads/keel-proof/x")
DEPLOY_KEY = os.environ.get("FLEET_TRAIN_DEPLOY_KEY", "")

# GitHub's push-rejection code for a repository RULESET violation. GH006 is
# the older branch-protection code and must not be accepted in its place:
# seeing GH006 here would mean the rejection came from something other than
# the ruleset this test exists to prove.
RULESET_REJECT_CODE = "GH013"


def _live() -> bool:
    return os.environ.get(LIVE_ENV) == "1"


def _git(args: list[str], cwd: Path, env_extra: dict | None = None) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    # Never hang waiting for a username/password: a missing credential must
    # be an rc, not a stalled test.
    env["GIT_TERMINAL_PROMPT"] = "0"
    env.setdefault("GIT_ASKPASS", "/usr/bin/false")
    if env_extra:
        env.update(env_extra)
    return subprocess.run(
        ["git", *args], cwd=str(cwd), capture_output=True, text=True, env=env, timeout=180
    )


class ProofRefCase(unittest.TestCase):
    """Shared fixture: a scratch repo whose only remote is the code repo."""

    ssh_env: dict = {}

    @classmethod
    def setUpClass(cls):
        if not _live():
            raise unittest.SkipTest(
                f"{LIVE_ENV}!=1 -- this test pushes to the real repo {CODE_URL}"
            )
        cls._tmp = tempfile.TemporaryDirectory(prefix="keel-proof-")
        cls.repo = Path(cls._tmp.name)
        assert str(cls.repo).startswith(tempfile.gettempdir()), cls.repo
        cls.evidence: list[dict] = []
        _git(["init", "-q", "-b", "proof"], cls.repo)
        _git(["config", "user.email", "keel-proof@invalid"], cls.repo)
        _git(["config", "user.name", "keel proof test"], cls.repo)
        _git(["remote", "add", "origin", CODE_URL], cls.repo)

    @classmethod
    def tearDownClass(cls):
        if hasattr(cls, "_tmp"):
            cls._tmp.cleanup()
        if getattr(cls, "evidence", None):
            print("\n=== instrument: test_tip_ruleset (live) ===")
            print(f"    repo={CODE_URL} ref={PROOF_REF}")
            for row in cls.evidence:
                print(f"    case={row['case']:<22} rc={row['rc']}")
                for line in row["stderr"].splitlines():
                    print(f"        | {line}")

    @classmethod
    def record(cls, case: str, proc: subprocess.CompletedProcess) -> subprocess.CompletedProcess:
        cls.evidence.append({"case": case, "rc": proc.returncode, "stderr": proc.stderr.strip()})
        return proc

    @classmethod
    def remote_sha(cls) -> str | None:
        out = _git(["ls-remote", "origin", PROOF_REF], cls.repo).stdout.strip()
        return out.split("\t")[0] if out else None

    @classmethod
    def commit_on_proof_tip(cls) -> str:
        """A new commit whose parent is the current remote proof ref (or a
        root commit if the ref does not exist yet) -- i.e. a fast-forward."""
        existing = cls.remote_sha()
        if existing:
            _git(["fetch", "-q", "origin", f"{PROOF_REF}:refs/remotes/proofref"], cls.repo)
            _git(["checkout", "-q", "--detach", existing], cls.repo)
        else:
            _git(["checkout", "-q", "--orphan", "proof-root"], cls.repo)
        _git(
            ["commit", "-q", "--allow-empty", "-m", f"keel ruleset proof {int(time.time())}"],
            cls.repo,
        )
        return _git(["rev-parse", "HEAD"], cls.repo).stdout.strip()

    @classmethod
    def unrelated_commit(cls) -> str:
        """A root commit sharing no history with the proof ref, so pushing it
        is a non-fast-forward no matter what the ref currently points at --
        including on the very first run, where `HEAD^` does not exist."""
        _git(["checkout", "-q", "--orphan", f"unrelated-{int(time.time()*1000)}"], cls.repo)
        _git(["commit", "-q", "--allow-empty", "-m", "unrelated root"], cls.repo)
        return _git(["rev-parse", "HEAD"], cls.repo).stdout.strip()


class TestKeylessMatrix(ProofRefCase):
    """The three cases the guard-only stage can prove, in order: each later
    case needs the ref that the earlier one created."""

    def test_1_keyless_fast_forward_push_is_allowed(self):
        sha = self.commit_on_proof_tip()
        proc = self.record(
            "keyless-ff", _git(["push", "origin", f"{sha}:{PROOF_REF}"], self.repo, self.ssh_env)
        )
        self.assertEqual(
            proc.returncode, 0,
            "a fast-forward push to a GUARD-ONLY proof ref must succeed "
            "(deletion/non_fast_forward do not restrict updates). "
            f"If stderr carries {RULESET_REJECT_CODE}, a restrict-updates "
            "ruleset (proof-update) has been applied to this ref and this "
            f"expectation must flip to rc!=0.\nstderr:\n{proc.stderr}",
        )
        self.assertEqual(self.remote_sha(), sha)

    def test_2_keyless_force_push_is_rejected_with_gh013(self):
        sha = self.unrelated_commit()
        proc = self.record(
            "keyless-force",
            _git(["push", "--force", "origin", f"{sha}:{PROOF_REF}"], self.repo, self.ssh_env),
        )
        self.assertNotEqual(proc.returncode, 0, f"force push was ACCEPTED\nstderr:\n{proc.stderr}")
        self.assertIn(
            RULESET_REJECT_CODE, proc.stderr,
            "rejected, but not by a ruleset -- GH013 absent, so this rc "
            f"proves nothing about proof-guard.\nstderr:\n{proc.stderr}",
        )
        self.assertNotEqual(self.remote_sha(), sha, "the ref moved despite the rejection")

    def test_3_keyless_delete_is_rejected(self):
        before = self.remote_sha()
        self.assertIsNotNone(before, "nothing to delete -- case 1 must run first")
        proc = self.record(
            "keyless-delete", _git(["push", "origin", "--delete", PROOF_REF], self.repo, self.ssh_env)
        )
        self.assertNotEqual(proc.returncode, 0, f"delete was ACCEPTED\nstderr:\n{proc.stderr}")
        self.assertIn(RULESET_REJECT_CODE, proc.stderr, f"stderr:\n{proc.stderr}")
        self.assertEqual(self.remote_sha(), before, "the ref was deleted despite the rejection")


@unittest.skipUnless(
    DEPLOY_KEY and Path(DEPLOY_KEY).is_file(),
    "FLEET_TRAIN_DEPLOY_KEY unset -- the keel-train deploy key is a human "
    "precondition (SPEC §8); see rulesets.py TODO(keel-train)",
)
class TestDeployKeyMatrix(ProofRefCase):
    """PLAN Stage 1's remaining acceptance rows, unlocked by the deploy key.

    With `proof-update` applied, the deploy key is the only principal that
    may advance the ref, and `proof-guard` -- which has NO bypass actors --
    still denies it `--force` and `--delete`. That asymmetry is the whole
    reason the pair is two rulesets rather than one.
    """

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        # The documented 1Password-agent bypass (SPEC §8): IdentitiesOnly +
        # IdentityAgent=none, or ssh offers the agent's keys instead.
        cls.ssh_env = {
            "GIT_SSH_COMMAND": (
                f"ssh -i {DEPLOY_KEY} -o IdentitiesOnly=yes -o IdentityAgent=none"
            )
        }

    def test_1_deploy_key_fast_forward_push_is_allowed(self):
        sha = self.commit_on_proof_tip()
        proc = self.record(
            "key-ff", _git(["push", "origin", f"{sha}:{PROOF_REF}"], self.repo, self.ssh_env)
        )
        self.assertEqual(proc.returncode, 0, f"stderr:\n{proc.stderr}")

    def test_2_deploy_key_force_push_is_rejected(self):
        sha = self.unrelated_commit()
        proc = self.record(
            "key-force",
            _git(["push", "--force", "origin", f"{sha}:{PROOF_REF}"], self.repo, self.ssh_env),
        )
        self.assertNotEqual(proc.returncode, 0, f"stderr:\n{proc.stderr}")
        self.assertIn(RULESET_REJECT_CODE, proc.stderr, f"stderr:\n{proc.stderr}")

    def test_3_deploy_key_delete_is_rejected(self):
        proc = self.record(
            "key-delete", _git(["push", "origin", "--delete", PROOF_REF], self.repo, self.ssh_env)
        )
        self.assertNotEqual(proc.returncode, 0, f"stderr:\n{proc.stderr}")
        self.assertIn(RULESET_REJECT_CODE, proc.stderr, f"stderr:\n{proc.stderr}")


if __name__ == "__main__":
    if not _live():
        print(
            f"{LIVE_ENV}!=1 -- refusing to push to {CODE_URL}. "
            f"Re-run with {LIVE_ENV}=1.",
            file=sys.stderr,
        )
        raise SystemExit(0)
    unittest.main(verbosity=2)
