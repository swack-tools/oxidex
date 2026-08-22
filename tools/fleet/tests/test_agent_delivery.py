#!/usr/bin/env python3
"""T5 -- the agent's work gets DELIVERED, and a failure to deliver it says
so.

THE BUG. `agentworker`'s task prompt ends in `git push origin
HEAD:refs/heads/staging/<slug> --force-with-lease` -- a real write to the
code repo, executed by a git the worker never spawns, inside a CLI the
worker only observes through three lines of captured output. That push
therefore ran under whatever credentials the box happened to have: on the
laptops the operator's osxkeychain entry, on a headless runner nothing at
all. Every other git command in `tools/fleet` goes through
`fleetlib.run_git` for exactly this reason (R5 did it for
`workqueue.Queue._git`); this one was invisible because it is not in this
repo's source at all, it is in a prompt.

And the way it failed is the expensive half. The worker's own instrument is
the code ref -- "the agent's word is not evidence" -- so a push that failed
to authenticate is indistinguishable from an agent that did nothing: the
branch sha is unchanged, and `run()` returns 7, `no-progress`. On a private
HTTPS spine with no credential that is the NORMAL outcome, on every branch,
forever: a host burning a full agent invocation per attempt and reporting
the same benign-sounding reason a genuinely stuck branch reports. Nothing
anywhere says "credential".

THE FIX, in three parts, all pinned below.

  1. `_agent_env()` is built from `fleetlib.credential_env()`, so the git
     the AGENT runs inherits the fleet's file-backed credential helper (and
     the `credential.helper=` reset that stops a host helper answering
     first).
  2. The worker DELIVERS the work itself, through `fleetlib.run_git`, after
     the CLI exits -- a no-op when the agent already pushed, and the only
     push at all when the agent could not.
  3. An authentication failure gets its own exit code
     (`RC_PUSH_AUTH_FAILED`, 10) and a message naming the token file, so
     `fleetd`'s ledger records `push-auth-failed` rather than
     `no-progress`.

Instrument: real `git` against real local bare repos under
`tempfile.gettempdir()`, with a stub "CLI" that makes a commit and does NOT
push (`FLEET_AGENT_CLI_OVERRIDE`). The auth failure is injected at
`fleetlib.run_git`'s return value rather than by trying to provoke a real
403, because what is under test is the CLASSIFICATION and the exit code,
and a fixture that needed a live remote to reach that code would not run in
the gate at all. `_is_auth_failure` is separately pinned against real git
and GitHub stderr text.

Run with:
    python3 -m unittest tools.fleet.tests.test_agent_delivery -v
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import agentworker  # noqa: E402
import fleetd  # noqa: E402
import fleetlib  # noqa: E402
from fleetlib import Hub  # noqa: E402

TIP_REF = "refs/heads/refactor/tag-machinery"
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}

# The stub headless CLI. It COMMITS and deliberately does NOT push, which
# is the state a real agent leaves behind when its own push fails -- and
# the state in which the pre-fix worker reported `no progress`.
_STUB_CLI = """#!/bin/bash
set -eu
printf 'converged onto the tip\\n' >> agent.txt
git add agent.txt
git -c user.email=a@a -c user.name=a commit -qm "agent: converge"
echo "CONVERGED stub"
"""

# Same, but it pushes too -- a well-behaved agent on a host whose
# credentials work. The worker's own delivery must be a silent no-op here.
_STUB_CLI_THAT_PUSHES = """#!/bin/bash
set -eu
printf 'converged onto the tip\\n' >> agent.txt
git add agent.txt
git -c user.email=a@a -c user.name=a commit -qm "agent: converge"
git push -q origin HEAD:refs/heads/staging/alpha --force-with-lease
echo "CONVERGED stub"
"""

# Commits, then declares BLOCKED -- the F1 shape (Stage 1d review). Before
# the fix the worker delivered this commit and reported CONVERGED, because
# the delivery ran ahead of the verdict. Writes its HEAD to a file so the
# test can assert the worker NAMED the sha it left behind.
_STUB_CLI_COMMIT_THEN_BLOCKED = """#!/bin/bash
set -eu
printf 'half-done\\n' >> agent.txt
git add agent.txt
git -c user.email=a@a -c user.name=a commit -qm "agent: partial work"
git rev-parse HEAD > {sha_file}
echo "BLOCKED: the second implementation needs a human"
"""


def _run(args, cwd=None, check=True):
    return subprocess.run(
        args, cwd=cwd, check=check, capture_output=True, text=True,
        env={**os.environ, **GIT_ENV},
    )


class _Repos:
    """`state.git` (coordination) and `code.git` (the tip plus a
    `staging/alpha` that has really drifted off it -- without the drift
    `dispatch.economic_refusal` refuses the run before it reaches anything
    under test here)."""

    def __init__(self, tmp: Path):
        assert str(tmp).startswith(tempfile.gettempdir())
        self.tmp = tmp
        self.state = tmp / "state.git"
        self.code = tmp / "code.git"
        self.work = tmp / "seed"
        _run(["git", "init", "-q", "--bare", str(self.state)])
        _run(["git", "init", "-q", "--bare", str(self.code)])
        _run(["git", "init", "-q", str(self.work)])
        self.tip_sha = self._seed()

    def _git(self, *args):
        return _run(["git", "-C", str(self.work), *args])

    def _seed(self) -> str:
        (self.work / "README").write_text("root\n")
        self._git("add", ".")
        self._git("commit", "-qm", "root")
        self._git("branch", "-M", "refactor/tag-machinery")
        self._git("push", "-q", str(self.code), "refactor/tag-machinery")

        self._git("checkout", "-q", "-b", "staging/alpha")
        (self.work / "alpha.txt").write_text("alpha\n")
        self._git("add", "alpha.txt")
        self._git("commit", "-qm", "alpha work")
        self.alpha_sha = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "staging/alpha:refs/heads/staging/alpha")

        self._git("checkout", "-q", "refactor/tag-machinery")
        (self.work / "tip2.txt").write_text("tip moved\n")
        self._git("add", "tip2.txt")
        self._git("commit", "-qm", "tip advances past alpha")
        tip = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "refactor/tag-machinery")
        return tip

    def sha_on_code(self, ref: str):
        out = _run(["git", "ls-remote", str(self.code), ref]).stdout.strip()
        return out.split("\t")[0] if out else None


class _WorkerCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        # HOME into the fixture: `agentworker.run` builds its Hub at
        # `Path.home()/.fleetd/agentcache`, and `fleetlib.credential_env`
        # falls back to `$HOME/.keel/secrets/git-token` (R2) -- a
        # provisioned host would otherwise wire its REAL PAT into these
        # fixtures.
        self._home = self.tmp / "home"
        self._home.mkdir()
        self._saved = {k: os.environ.get(k) for k in (
            "HOME", "FLEET_AGENT_CLI_OVERRIDE", "FLEET_GIT_TOKEN_FILE")}
        os.environ["HOME"] = str(self._home)
        os.environ.pop("FLEET_GIT_TOKEN_FILE", None)
        self.repos = _Repos(self.tmp)

    def tearDown(self):
        for key, value in self._saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value

    def install_cli(self, body: str = _STUB_CLI) -> Path:
        cli = self.tmp / "stub-cli.sh"
        cli.write_text(body)
        cli.chmod(0o755)
        os.environ["FLEET_AGENT_CLI_OVERRIDE"] = str(cli)
        return cli

    def run_worker(self):
        return agentworker.run("staging/alpha", str(self.repos.state), "t",
                               code_url=str(self.repos.code))


# --------------------------------------------------------------------- #
# The delivery itself
# --------------------------------------------------------------------- #


class TestTheWorkerDeliversWhatTheAgentCommitted(_WorkerCase):
    def test_a_commit_the_agent_did_not_push_still_lands(self):
        """The pre-fix run of this exact fixture returns 7 (`no progress`)
        and leaves `staging/alpha` where it was: the stub CLI commits and
        does not push, and nothing else in the worker ever pushed."""
        self.install_cli()
        rc = self.run_worker()
        self.assertEqual(rc, 0, "the worker did not deliver the agent's commit")
        landed = self.repos.sha_on_code("refs/heads/staging/alpha")
        self.assertNotEqual(landed, self.repos.alpha_sha,
                            "staging/alpha did not move on the code repo")

    def test_an_agent_that_pushed_for_itself_is_not_pushed_twice(self):
        """`git push` of a ref the remote already equals is
        "Everything up-to-date" and never consults the lease, so the
        fallback costs one round trip and changes nothing. Pinned because
        the obvious wrong implementation -- an unconditional `--force` --
        would also pass the test above while quietly discarding a commit
        pushed during the run."""
        self.install_cli(_STUB_CLI_THAT_PUSHES)
        rc = self.run_worker()
        self.assertEqual(rc, 0)
        landed = self.repos.sha_on_code("refs/heads/staging/alpha")
        self.assertNotEqual(landed, self.repos.alpha_sha)

    def test_an_agent_that_committed_nothing_pushes_nothing(self):
        """The negative control. A `_deliver` that pushed unconditionally
        would report success for a run in which the agent did nothing at
        all -- turning `no progress` into a false `converged`, which is
        strictly worse than the bug being fixed."""
        cli = self.tmp / "noop-cli.sh"
        cli.write_text("#!/bin/bash\necho 'BLOCKED: nothing to do'\n")
        cli.chmod(0o755)
        os.environ["FLEET_AGENT_CLI_OVERRIDE"] = str(cli)

        rc = self.run_worker()
        self.assertEqual(rc, agentworker.RC_BLOCKED)
        self.assertEqual(self.repos.sha_on_code("refs/heads/staging/alpha"),
                         self.repos.alpha_sha, "the branch moved with no agent commit")


class TestABlockedVerdictPushesNothing(_WorkerCase):
    """F1 (Stage 1d review). The T5 delivery used to run BEFORE the
    worker read the agent's verdict, so an agent that committed partial
    work and then correctly declared BLOCKED had that work delivered
    anyway -- and, the branch having moved, was reported as CONVERGED
    (rc 0), which resets fleetd's consecutive-failure count. The mutation
    table for T5's seams caught every other inversion; this one it could
    not, because no test committed AND said BLOCKED in the same run.

    `TestTheWorkerDeliversWhatTheAgentCommitted.test_a_commit_the_agent_did_not_push_still_lands`
    is the negative control: the SAME commit without the BLOCKED line IS
    delivered. The two differ only in the verdict, which is the point.
    """

    def test_an_agent_that_commits_then_declares_blocked_pushes_nothing(self):
        import contextlib
        import io

        sha_file = self.tmp / "blocked-head"
        self.install_cli(_STUB_CLI_COMMIT_THEN_BLOCKED.format(sha_file=sha_file))
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = self.run_worker()
        self.assertEqual(rc, agentworker.RC_BLOCKED, buf.getvalue())
        self.assertEqual(self.repos.sha_on_code("refs/heads/staging/alpha"),
                         self.repos.alpha_sha,
                         "a BLOCKED run moved the branch: the commit was delivered "
                         "ahead of the verdict")
        # The sha that was NOT delivered is named, so the log is the one
        # durable pointer to what the agent left behind.
        head = sha_file.read_text().strip()
        self.assertTrue(head)
        self.assertIn(head, buf.getvalue())
        self.assertIn("pushing nothing", buf.getvalue())

    def test_the_same_commit_without_the_verdict_is_delivered(self):
        """The negative control, restated beside the test it controls for:
        a worker that never pushed at all would pass the test above."""
        self.install_cli()  # _STUB_CLI: the same commit, final line CONVERGED
        self.assertEqual(self.run_worker(), 0)
        self.assertNotEqual(self.repos.sha_on_code("refs/heads/staging/alpha"),
                            self.repos.alpha_sha)


# --------------------------------------------------------------------- #
# An auth failure is its own outcome
# --------------------------------------------------------------------- #


class TestPushAuthFailureIsItsOwnExitCode(_WorkerCase):
    """`fleetd`'s attempt ledger resets a branch's consecutive-failure
    count on some outcomes and not others, so "which outcome" is not
    cosmetic. `no-progress` says the branch is hard; `push-auth-failed`
    says the HOST cannot write, which no amount of re-buying the branch
    will fix and which a human has to resolve.
    """

    @staticmethod
    def _fail_the_push_with(stderr: str):
        """Wrap `fleetlib.run_git` so that only the delivery push fails,
        with `stderr`. Everything else -- clone, checkout, fetch, the ref
        probes -- runs for real, so the run reaches the push the same way
        production does."""
        real = fleetlib.run_git

        def wrapper(cmd, **kw):
            if "push" in cmd:
                return fleetlib._Result(returncode=128, stdout="", stderr=stderr,
                                        args=list(cmd[1:]))
            return real(cmd, **kw)

        return wrapper

    def test_an_authentication_failure_returns_its_own_code(self):
        self.install_cli()
        stderr = ("remote: Invalid username or password.\n"
                  "fatal: Authentication failed for "
                  "'https://github.com/swack-tools/oxidex.git/'\n")
        with mock.patch.object(agentworker.fleetlib, "run_git",
                               self._fail_the_push_with(stderr)):
            rc = self.run_worker()
        self.assertEqual(rc, agentworker.RC_PUSH_AUTH_FAILED)
        self.assertNotEqual(rc, 7, "an auth failure must not read as 'no progress'")

    def test_a_non_auth_push_failure_is_not_reported_as_an_auth_failure(self):
        """The negative control, and the one that keeps the new code
        honest: a lost `--force-with-lease` race is a CONTENT failure --
        somebody pushed during the run -- and must keep reading as "the
        branch did not move", not as a credential problem a human is asked
        to go fix."""
        self.install_cli()
        stderr = ("! [rejected]  HEAD -> staging/alpha (stale info)\n"
                  "error: failed to push some refs\n")
        with mock.patch.object(agentworker.fleetlib, "run_git",
                               self._fail_the_push_with(stderr)):
            rc = self.run_worker()
        self.assertEqual(rc, 7)
        self.assertNotEqual(rc, agentworker.RC_PUSH_AUTH_FAILED)

    def test_the_classifier_recognises_what_git_and_github_actually_say(self):
        """Real stderr text, not paraphrases -- the classifier's whole job
        is matching strings this fleet will really see."""
        for text in (
            "fatal: Authentication failed for 'https://github.com/o/r.git/'",
            "fatal: could not read Username for 'https://github.com': "
            "terminal prompts disabled",
            "remote: Permission to o/r.git denied to deploy-key.",
            "remote: Write access to repository not granted.",
            "The requested URL returned error: 403",
            "remote: Support for password authentication was removed on "
            "August 13, 2021.",
        ):
            with self.subTest(text=text[:40]):
                self.assertTrue(agentworker._is_auth_failure(text))

    def test_the_classifier_does_not_claim_content_failures(self):
        for text in (
            "! [rejected] main -> main (non-fast-forward)",
            "error: failed to push some refs to 'origin'",
            "hint: Updates were rejected because the remote contains work",
            "fatal: couldn't find remote ref refs/heads/nope",
            "! [rejected] HEAD -> staging/alpha (stale info)",
        ):
            with self.subTest(text=text[:40]):
                self.assertFalse(agentworker._is_auth_failure(text))

    def test_fleetd_has_a_name_for_the_code(self):
        """An exit code fleetd renders as `exit-10` is a number in a log;
        the ledger and `fleet status` both read the NAME."""
        self.assertEqual(fleetd._AGENT_RC_OUTCOMES[agentworker.RC_PUSH_AUTH_FAILED],
                         "push-auth-failed")
        self.assertNotIn(agentworker.RC_PUSH_AUTH_FAILED, (0, 7, 8, 9))


# --------------------------------------------------------------------- #
# Everything git the worker runs carries the fleet environment
# --------------------------------------------------------------------- #


class TestWorkerGitGoesThroughFleetlib(_WorkerCase):
    def _spy(self):
        calls = []
        real = fleetlib.subprocess.run

        def spy(cmd, **kw):
            calls.append({"argv": list(cmd), "env": kw.get("env")})
            return real(cmd, **kw)

        return calls, spy

    def test_every_git_command_the_worker_spawns_carries_the_pinned_transport(self):
        """Including the local ones. "Which of these talks to a remote" is
        precisely the judgement that was wrong the first time (R5's own
        note), so the fence is all of them.

        The instrument is the `env=` dict actually handed to
        `subprocess.run` -- the only observation that distinguishes
        "routed through fleetlib" from "looks routed".
        """
        self.install_cli()
        calls, spy = self._spy()
        with mock.patch.object(fleetlib.subprocess, "run", spy):
            rc = self.run_worker()
        self.assertEqual(rc, 0)

        stub = os.environ["FLEET_AGENT_CLI_OVERRIDE"]
        gits = [c for c in calls
                if c["argv"] and c["argv"][0] == "git" and c["argv"][0] != stub]
        # NOTE the fence is every git this process spawned during the run,
        # not only the ones `agentworker.py` itself writes: `preflight` ->
        # `dispatch.economic_refusal` spawns three more (a FETCH from
        # `hub.code_url`, `merge-base`, `merge-tree`) through
        # `dispatch._git`, which was a bare `subprocess.run` until this
        # test found it. Narrowing the fence to agentworker's own argv
        # shapes would have made it pass while leaving the remote fetch
        # unauthenticated -- the filter eating the answer.
        self.assertTrue(gits, "the worker spawned no git at all")
        for call in gits:
            with self.subTest(argv=" ".join(call["argv"][:4])):
                self.assertIsNotNone(
                    call["env"],
                    "a bare subprocess.run(['git', ...]) is back: no credential "
                    "helper, no pinned ssh options, no GIT_TERMINAL_PROMPT=0")
                self.assertEqual(call["env"].get("GIT_SSH_COMMAND"),
                                 fleetlib.DEFAULT_SSH_COMMAND)
                self.assertEqual(call["env"].get("GIT_TERMINAL_PROMPT"), "0")

    def test_the_clone_is_one_of_them(self):
        """Named separately because it is the one that talks to the code
        remote first, and the one whose failure used to be a
        `CalledProcessError` traceback rather than an exit code."""
        self.install_cli()
        calls, spy = self._spy()
        with mock.patch.object(fleetlib.subprocess, "run", spy):
            self.run_worker()
        clones = [c for c in calls if c["argv"][:2] == ["git", "clone"]]
        self.assertEqual(len(clones), 1, [c["argv"][:3] for c in calls])
        self.assertIsNotNone(clones[0]["env"])
        self.assertIn(str(self.repos.code), clones[0]["argv"])


class TestAgentEnvironmentCarriesTheCredential(_WorkerCase):
    """Part 1 of the fix: the git the AGENT runs, which this repo never
    spawns and cannot wrap, still gets the fleet credential -- because the
    environment it is started under carries it."""

    def _token_file(self) -> Path:
        secrets = self._home / ".keel" / "secrets"
        secrets.mkdir(parents=True)
        token = secrets / "git-token"
        token.write_text("ghp_fixture\n")
        token.chmod(0o600)
        return token

    def test_the_helper_reaches_the_agents_environment(self):
        self._token_file()
        env = agentworker._agent_env()
        keys = {env[k]: env.get(k.replace("KEY", "VALUE"))
                for k in env if k.startswith("GIT_CONFIG_KEY_")}
        self.assertIn("credential.helper", keys,
                      "the agent's git would fall through to the host helper")
        # Both entries: the empty-valued reset AND ours. Without the reset,
        # osxkeychain (or the 1Password agent) answers first and the PAT is
        # never consulted.
        helper_values = [env[k.replace("KEY", "VALUE")] for k in env
                         if k.startswith("GIT_CONFIG_KEY_")
                         and env[k] == "credential.helper"]
        self.assertIn("", helper_values)
        self.assertTrue(any(v.startswith("!") for v in helper_values), helper_values)
        self.assertEqual(env.get("FLEET_GIT_TOKEN_FILE"),
                         str(self._home / ".keel" / "secrets" / "git-token"))

    def test_path_is_still_the_fleet_path(self):
        """The property `_agent_env` had before and must keep: a daemon's
        minimal PATH misses `~/.local/bin` (claude) and the nvm bin
        (codex)."""
        self.assertEqual(agentworker._agent_env()["PATH"], agentworker.FLEET_PATH)

    def test_no_token_file_means_an_unchanged_environment(self):
        """"Unset changes nothing" -- the property that keeps every
        ssh-spine host and every `git init --bare` fixture running exactly
        the environment they ran before."""
        env = agentworker._agent_env()
        self.assertFalse([k for k in env if k.startswith("GIT_CONFIG_KEY_")])


# --------------------------------------------------------------------- #
# _deliver in isolation
# --------------------------------------------------------------------- #


class TestDeliverTargetsThePushUrl(_WorkerCase):
    def test_the_push_goes_to_code_push_url_not_code_url(self):
        """`Hub.code_push_url` defaults to `code_url`, so this is a no-op
        on today's single-URL fleet -- and is the difference between
        correct and wrong the moment reads and writes get different
        remotes, which SPEC §4.4 already provides for."""
        push_target = self.tmp / "code-push.git"
        _run(["git", "init", "-q", "--bare", str(push_target)])
        # Seeded to the same `staging/alpha` the read remote has: in
        # production the two URLs are two transports onto ONE repo, and a
        # `--force-with-lease` against a remote that has never had the ref
        # would lose on "stale info" for a reason that has nothing to do
        # with routing.
        _run(["git", "-C", str(self.repos.work), "push", "-q", str(push_target),
              f"{self.repos.alpha_sha}:refs/heads/staging/alpha"])
        hub = Hub(str(self.repos.state), workdir=self.tmp / "hc",
                  code_url=str(self.repos.code), code_push_url=str(push_target))

        clone = self.tmp / "clone"
        _run(["git", "clone", "-q", str(self.repos.code), str(clone)])
        _run(["git", "-C", str(clone), "checkout", "-q", "-B", "agent-work",
              self.repos.alpha_sha])
        (clone / "more.txt").write_text("more\n")
        _run(["git", "-C", str(clone), "add", "more.txt"])
        _run(["git", "-C", str(clone), "commit", "-qm", "agent work"])
        head = _run(["git", "-C", str(clone), "rev-parse", "HEAD"]).stdout.strip()

        result = agentworker._deliver(hub, clone, "staging/alpha",
                                      self.repos.alpha_sha, self.repos.alpha_sha)
        self.assertEqual(result[0], 0, result)
        on_push_target = _run(
            ["git", "ls-remote", str(push_target), "refs/heads/staging/alpha"]).stdout
        self.assertIn(head, on_push_target)
        # ...and NOT on the read remote.
        self.assertEqual(self.repos.sha_on_code("refs/heads/staging/alpha"),
                         self.repos.alpha_sha)

    def test_the_push_is_a_hub_write_from_the_hub_cache_not_from_the_clone(self):
        """ARCH-FIX R9: every branch write goes through `fleetlib.Hub`.
        `tests/test_no_raw_hub_push.py` greps the SOURCE for a raw
        `"push"` argv element and caught the first version of `_deliver`
        (a `git -C <clone> push ...`); this pins the BEHAVIOUR the grep
        stands in for, with the argv actually handed to `subprocess.run`
        as the instrument: the one push is `git --git-dir <hub.workdir>
        push ...` (the hub's own cache, the way `train`'s `rescued/*` push
        works), it carries the lease on the pre-run head, and nothing
        pushes from the clone."""
        hub = Hub(str(self.repos.state), workdir=self.tmp / "hc4",
                  code_url=str(self.repos.code))
        clone = self.tmp / "clone4"
        _run(["git", "clone", "-q", str(self.repos.code), str(clone)])
        _run(["git", "-C", str(clone), "checkout", "-q", "-B", "agent-work",
              self.repos.alpha_sha])
        (clone / "agent.txt").write_text("agent\n")
        _run(["git", "-C", str(clone), "add", "agent.txt"])
        _run(["git", "-C", str(clone), "commit", "-qm", "agent work"])

        calls = []
        real = fleetlib.subprocess.run

        def spy(cmd, **kw):
            calls.append(list(cmd))
            return real(cmd, **kw)

        with mock.patch.object(fleetlib.subprocess, "run", spy):
            rc, _detail = agentworker._deliver(
                hub, clone, "staging/alpha", self.repos.alpha_sha, self.repos.alpha_sha)
        self.assertEqual(rc, 0)
        pushes = [c for c in calls if c and c[0] == "git" and "push" in c]
        self.assertEqual(len(pushes), 1, pushes)
        argv = pushes[0]
        self.assertEqual(argv[:3], ["git", "--git-dir", str(hub.workdir)], argv)
        self.assertNotIn("-C", argv, "the push ran from the clone, not the hub cache")
        self.assertIn(f"--force-with-lease=refs/heads/staging/alpha:{self.repos.alpha_sha}",
                      argv)
        self.assertNotIn("--force", argv)

    def test_nothing_to_push_is_none_not_a_failure(self):
        hub = Hub(str(self.repos.state), workdir=self.tmp / "hc2",
                  code_url=str(self.repos.code))
        clone = self.tmp / "clone2"
        _run(["git", "clone", "-q", str(self.repos.code), str(clone)])
        _run(["git", "-C", str(clone), "checkout", "-q", "-B", "agent-work",
              self.repos.alpha_sha])
        self.assertIsNone(agentworker._deliver(
            hub, clone, "staging/alpha", self.repos.alpha_sha, self.repos.alpha_sha))

    def test_the_lease_protects_a_commit_pushed_during_the_run(self):
        """`--force-with-lease=<ref>:<before_sha>`, never a bare `--force`.
        The agent window is up to an hour; a raw force here discards
        whatever the author pushed during it, with no trace -- the same
        rule, for the same reason, as `train._retire_staging_ref`'s CAS."""
        hub = Hub(str(self.repos.state), workdir=self.tmp / "hc3",
                  code_url=str(self.repos.code))
        clone = self.tmp / "clone3"
        _run(["git", "clone", "-q", str(self.repos.code), str(clone)])
        _run(["git", "-C", str(clone), "checkout", "-q", "-B", "agent-work",
              self.repos.alpha_sha])
        (clone / "agent.txt").write_text("agent\n")
        _run(["git", "-C", str(clone), "add", "agent.txt"])
        _run(["git", "-C", str(clone), "commit", "-qm", "agent work"])

        # Somebody else moves staging/alpha while the agent was working.
        other = self.tmp / "other"
        _run(["git", "clone", "-q", str(self.repos.code), str(other)])
        _run(["git", "-C", str(other), "checkout", "-q", "-B", "x", self.repos.alpha_sha])
        (other / "author.txt").write_text("the author's own commit\n")
        _run(["git", "-C", str(other), "add", "author.txt"])
        _run(["git", "-C", str(other), "commit", "-qm", "author pushed during the run"])
        _run(["git", "-C", str(other), "push", "-q", str(self.repos.code),
              "HEAD:refs/heads/staging/alpha", "--force"])
        moved = self.repos.sha_on_code("refs/heads/staging/alpha")

        rc, detail = agentworker._deliver(
            hub, clone, "staging/alpha", self.repos.alpha_sha, self.repos.alpha_sha)
        self.assertNotEqual(rc, 0, "the lease did not refuse a lost race")
        self.assertNotEqual(rc, agentworker.RC_PUSH_AUTH_FAILED)
        self.assertEqual(self.repos.sha_on_code("refs/heads/staging/alpha"), moved,
                         "the author's commit was overwritten")


class TestPushCodeRefLease(_WorkerCase):
    """`Hub.push_code_ref(force_with_lease=<sha>)`, the primitive
    `_deliver` routes through: the lease is on the refspec's DESTINATION
    ref, a stale lease loses without moving the ref, a current one wins a
    non-fast-forward, and it cannot be combined with the bare `--force`
    it exists to replace."""

    def _hub(self, name):
        hub = Hub(str(self.repos.state), workdir=self.tmp / name,
                  code_url=str(self.repos.code))
        # push_code_ref pushes from the hub cache, so the tip must be there.
        _run(["git", "--git-dir", str(hub.workdir), "fetch", "--quiet",
              str(self.repos.code), TIP_REF])
        return hub

    def test_force_and_force_with_lease_are_mutually_exclusive(self):
        hub = self._hub("lease0")
        with self.assertRaises(ValueError):
            hub.push_code_ref(f"{self.repos.tip_sha}:refs/heads/staging/alpha",
                              force=True, force_with_lease=self.repos.alpha_sha)

    def test_a_lease_needs_a_destination_ref(self):
        hub = self._hub("lease1")
        with self.assertRaises(ValueError):
            hub.push_code_ref(f"{self.repos.tip_sha}:", force_with_lease=self.repos.alpha_sha)

    def test_a_stale_lease_loses_and_a_current_one_wins(self):
        hub = self._hub("lease2")
        # staging/alpha has diverged from the tip, so moving it TO the tip
        # is a non-fast-forward: without a (winning) lease git refuses.
        stale = hub.push_code_ref(f"{self.repos.tip_sha}:refs/heads/staging/alpha",
                                  force_with_lease="0" * 40)
        self.assertNotEqual(stale.returncode, 0)
        self.assertEqual(self.repos.sha_on_code("refs/heads/staging/alpha"),
                         self.repos.alpha_sha, "a stale lease moved the ref")

        ok = hub.push_code_ref(f"{self.repos.tip_sha}:refs/heads/staging/alpha",
                               force_with_lease=self.repos.alpha_sha)
        self.assertEqual(ok.returncode, 0, ok.stderr)
        self.assertEqual(self.repos.sha_on_code("refs/heads/staging/alpha"),
                         self.repos.tip_sha)


if __name__ == "__main__":
    unittest.main()
