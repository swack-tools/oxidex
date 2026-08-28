#!/usr/bin/env python3
"""T1/T3/T4 -- the train against THREE distinct repos, which is the only
shape in which the tip's routing is observable at all.

WHY THIS FILE EXISTS. `docs/AGENT-SERVER-SPEC.md` §4.4 splits the fleet
spine four ways: coordination on `hub.url`, code reads on `hub.code_url`,
code writes on `hub.code_push_url`, and THE TIP -- and only the tip -- on
`hub.tip_push_url`, because the tip is the one write whose credential is
the deploy key rather than the host PAT. `test_code_url_split.py` pins that
routing at the `fleetlib.Hub` method level (`push_tip_ref` lands here,
`push_code_ref` lands there) and runs a real train landing with
`code_push_url` and `tip_push_url` pointing at the SAME repo.

That last part is the hole this file closes. With one URL behind both
methods, a train that sent `rescued/*`, the `staging/*` retirement and
`staging/train-tmp-*` through `push_tip_ref` would put every ref exactly
where the existing tests look for it and pass the whole suite -- and would,
in production, push three PAT-authenticated code writes over ssh under
whatever key the ambient agent offered. The routing was pinned; the
train's USE of the routing was not.

So: three real bare repos at three distinct local paths, and one real
`run_train` landing across them.

  * `state.git`  -- coordination. Gets `refs/fleet/*` and nothing else.
  * `code.git`   -- the HTTPS/PAT stand-in. Gets `rescued/*`, the
                    `staging/*` retirement, `staging/train-tmp-*`.
  * `tip.git`    -- the ssh/deploy-key stand-in. Gets the tip advance and
                    NOTHING else.

`tip.git` is seeded as a MIRROR of `code.git`, which is what makes the
assertions sharp: both repos start with byte-identical `refs/heads/*`, so
"the tip advanced here and not there" is a statement about where the push
went, not about which repo happened to have the objects. In production the
two are one GitHub repo reached over two transports, and mirroring is the
closest a local fixture gets to that.

THE NEGATIVE CONTROLS ARE THE POINT (both are tests, not comments).
`test_negative_control_*` deliberately reintroduce each mis-routing and
assert that `assert_routing()` -- the exact helper the positive test uses
-- RAISES. A fixture that cannot fail is not evidence, and the way this
class of test dies is by growing a repo shape in which every path leads to
the same place.

Instrument: plain `unittest`; every observation is `git ls-remote` against
one of the three bare repos, run by the test rather than by the code under
test. Every repo lives under `tempfile.gettempdir()`; `HOME` is redirected
into the fixture (see `_ThreeRepoCase.setUp`) so `~/.fleetd`, `~/gatelogs`
and the R2 default PAT path can never be the developer's real ones.

Run with:
    python3 -m unittest tools.fleet.tests.test_train_three_repos -v
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

import config  # noqa: E402
import fleetlib  # noqa: E402
import train  # noqa: E402
from fleetlib import Hub  # noqa: E402
from _env import HermeticCase, scrub_env  # noqa: E402

TIP_REF = "refs/heads/refactor/tag-machinery"
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}

# A stand-in for `tools/fleet/gate.sh`, committed INTO the fixture's code
# tree because `train.real_gate` runs `<clone>/tools/fleet/gate.sh` from
# whatever tree it just merged. It writes the one artefact `real_gate`
# reads -- `$HOME/gatelogs/gate-<tag>.verdict` -- and, when
# `FIXTURE_GATE_MARK_STORE_FAILED` is set, the verdict-store-failure marker
# beside it, exactly as the real `store_verdict()` does on a hub-push
# failure. Nothing else about the real gate is simulated: this file is
# about ref routing and marker plumbing, not about gating.
_STUB_GATE_SH = """#!/bin/bash
set -eu
BRANCH="$1"; TAG="$2"
mkdir -p "$HOME/gatelogs"
echo "PASS stub-gate $BRANCH" > "$HOME/gatelogs/gate-$TAG.verdict"
if [ -n "${FIXTURE_GATE_MARK_STORE_FAILED:-}" ]; then
  : > "$HOME/gatelogs/gate-$TAG${FIXTURE_MARKER_SUFFIX}"
fi
"""


def _run(args, cwd=None, check=True):
    return subprocess.run(
        args, cwd=cwd, check=check, capture_output=True, text=True,
        env=scrub_env(**GIT_ENV),
    )


class _ThreeRepos:
    """`state.git`, `code.git` and `tip.git` as three distinct local bare
    repos, plus the seed worktree used to build the code history.

    `tip.git` starts as a mirror of `code.git`: same objects, same
    `refs/heads/*`. Anything that differs between them afterwards was put
    there by the run under test.
    """

    def __init__(self, tmp: Path):
        assert str(tmp).startswith(tempfile.gettempdir()), "fixture must live under tempdir"
        self.tmp = tmp
        self.state = tmp / "state.git"
        self.code = tmp / "code.git"
        self.tip = tmp / "tip.git"
        self.work = tmp / "seed"
        for bare in (self.state, self.code, self.tip):
            _run(["git", "init", "-q", "--bare", str(bare)])
        _run(["git", "init", "-q", str(self.work)])
        self.tip_sha = self._seed_code()
        # The mirror. `--mirror` copies refs/heads/* verbatim, so the two
        # code-side repos are indistinguishable until the run under test
        # writes to one of them.
        _run(["git", "-C", str(self.work), "push", "--quiet", "--mirror", str(self.tip)])

    def _git(self, *args, check=True):
        return _run(["git", "-C", str(self.work), *args], check=check)

    def _seed_code(self) -> str:
        (self.work / "domains.toml").write_text('[[domain]]\nname = "root"\n')
        gate = self.work / "tools" / "fleet"
        gate.mkdir(parents=True)
        (gate / "gate.sh").write_text(
            _STUB_GATE_SH.replace(
                "${FIXTURE_MARKER_SUFFIX}", config.VERDICT_STORE_FAILED_SUFFIX
            )
        )
        self._git("add", ".")
        self._git("commit", "-qm", "root")
        self._git("branch", "-M", "refactor/tag-machinery")
        self._git("push", "-q", str(self.code), "refactor/tag-machinery")

        # staging/alpha: real drift off the tip.
        self._git("checkout", "-q", "-b", "staging/alpha")
        (self.work / "alpha.txt").write_text("alpha\n")
        self._git("add", "alpha.txt")
        self._git("commit", "-qm", "alpha work")
        self.alpha_sha = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "staging/alpha:refs/heads/staging/alpha")

        # Then the tip moves on its own, so alpha's merge-base is NOT the
        # tip -- without this the batch is empty and there is nothing to
        # route anywhere (the same trap `test_code_url_split.py` documents).
        self._git("checkout", "-q", "refactor/tag-machinery")
        (self.work / "tip2.txt").write_text("tip moved on\n")
        self._git("add", "tip2.txt")
        self._git("commit", "-qm", "tip advances past alpha")
        tip = self._git("rev-parse", "HEAD").stdout.strip()
        self._git("push", "-q", str(self.code), "refactor/tag-machinery")
        return tip

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


class _ThreeRepoCase(HermeticCase):
    def setUp(self):
        super().setUp()
        self._tmpdir = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmpdir.name)
        # HOME is redirected for the same three reasons test_code_url_split
        # gives: `train.run_train`'s `~/.fleetd/traincache` default,
        # `real_gate`'s `~/gatelogs`, and `fleetlib.credential_env`'s R2
        # fallback to `$HOME/.keel/secrets/git-token` (a provisioned host
        # would otherwise wire its REAL PAT helper into this fixture).
        self._home = self.tmp / "home"
        self._home.mkdir()
        self._saved = {k: os.environ.get(k) for k in (
            "HOME", train.TRAIN_TOKEN_ENV, train.TRAIN_DEPLOY_KEY_ENV,
            "GIT_SSH_COMMAND", "FIXTURE_GATE_MARK_STORE_FAILED",
        )}
        os.environ["HOME"] = str(self._home)
        os.environ[train.TRAIN_TOKEN_ENV] = str(self.tmp / "no-such-train.token")
        for key in (train.TRAIN_DEPLOY_KEY_ENV, "GIT_SSH_COMMAND",
                    "FIXTURE_GATE_MARK_STORE_FAILED"):
            os.environ.pop(key, None)
        self.repos = _ThreeRepos(self.tmp)

    def tearDown(self):
        for key, value in self._saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
        self._tmpdir.cleanup()

    # -- the run under test -------------------------------------------- #

    def run_train(self, *, tip_push_url=None, gate_fn=None, **kw):
        """One real `train.run_train` across the three repos.

        `tip_push_url` defaults to `tip.git`; the negative control passes
        `code.git` to prove the fixture can tell the two apart.
        """
        return train.run_train(
            str(self.repos.state),
            self.tmp,
            gate_fn=gate_fn or (lambda clone, label: "PASS"),
            epoch="three-repo",
            hub_workdir=self.tmp / "traincache",
            code_url=str(self.repos.code),
            code_push_url=str(self.repos.code),
            tip_push_url=str(tip_push_url or self.repos.tip),
            **kw,
        )

    # -- the assertion the negative controls must be able to break ------ #

    def assert_routing(self, res, *, expect_tmp_refs=None):
        """SPEC §4.4's four columns, asserted against three real repos.

        Deliberately ONE helper used by the positive test and by both
        negative controls: a routing assertion that only ever runs in the
        passing configuration cannot be shown to discriminate, and the two
        `assertRaises(AssertionError)` tests below are how it is shown.
        """
        code = self.repos.refs_on(self.repos.code, "refs/heads/*")
        tip = self.repos.refs_on(self.repos.tip, "refs/heads/*")
        new_tip = res.new_tip
        self.assertTrue(new_tip, f"the run never advanced the tip: {res}")

        # 1. THE TIP -> tip_push_url, and nowhere else. Both halves matter:
        #    a train that pushed the tip to BOTH repos would satisfy the
        #    first line alone.
        self.assertEqual(tip.get(TIP_REF), new_tip,
                         "the tip did not advance on the tip (deploy-key) repo")
        self.assertEqual(code.get(TIP_REF), self.repos.tip_sha,
                         "the tip advanced on the CODE repo -- that push is the one "
                         "the deploy key exists for and it went to the PAT remote")

        # 2. rescued/* -> code_push_url, and NOT to the tip remote.
        self.assertEqual(code.get("refs/heads/rescued/alpha"), self.repos.alpha_sha,
                         "rescued/alpha is missing from the code (PAT) repo")
        self.assertNotIn("refs/heads/rescued/alpha", tip,
                         "rescued/alpha went to the tip (deploy-key) remote")

        # 3. the staging RETIREMENT is a code write: it deletes the branch
        #    on the code repo and leaves the tip repo's copy alone.
        self.assertNotIn("refs/heads/staging/alpha", code,
                         "staging/alpha was not retired on the code repo")
        self.assertIn("refs/heads/staging/alpha", tip,
                      "the retirement CAS ran against the tip (deploy-key) remote")

        # 4. coordination -> state repo, and nothing code-shaped anywhere
        #    near it. `refs/fleet/*` carries user@host:pid provenance
        #    (SPEC §8) and the code repo is PUBLIC.
        signal = self.repos.refs_on(self.repos.state, "refs/fleet/signals/*")
        self.assertIn("refs/fleet/signals/tip", signal)
        hub = Hub(str(self.repos.state), workdir=self.tmp / "verify")
        self.assertEqual((hub.read("refs/fleet/signals/tip") or {}).get("sha"), new_tip)
        self.assertEqual(self.repos.refs_on(self.repos.state, "refs/heads/*"), {})
        self.assertEqual(self.repos.refs_on(self.repos.code, "refs/fleet/*"), {})
        self.assertEqual(self.repos.refs_on(self.repos.tip, "refs/fleet/*"), {})

        # 5. `staging/train-tmp-*`, observed DURING the gate (it is deleted
        #    in `real_gate`'s `finally`). Only checked when the caller
        #    captured it -- the plain-`gate_fn` runs never push one.
        if expect_tmp_refs is not None:
            self.assertTrue(expect_tmp_refs["code"],
                            "no staging/train-tmp-* ref was ever seen on the code repo")
            self.assertEqual(expect_tmp_refs["tip"], {},
                             "the temp gate ref went to the tip (deploy-key) remote")
            self.assertEqual(expect_tmp_refs["state"], {},
                             "the temp gate ref went to the STATE repo")


# --------------------------------------------------------------------- #
# T1 -- one real landing, three repos
# --------------------------------------------------------------------- #


class TestTrainLandingAcrossThreeRepos(_ThreeRepoCase):
    def test_every_ref_lands_on_exactly_the_repo_that_owns_it(self):
        res = self.run_train()
        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")
        self.assertEqual(res.landed, ["staging/alpha"])
        self.assertNotEqual(res.new_tip, self.repos.tip_sha)
        self.assert_routing(res)

    def test_the_temp_gate_ref_is_a_code_write_not_a_tip_write(self):
        """`train.real_gate` pushes `staging/train-tmp-<tag>` before running
        gate.sh and CAS-deletes it afterwards, so the ref exists only
        during the gate. The observation is taken from inside
        `_delete_code_ref_cas`, which runs while the ref is still there --
        reading the repos after the run would see nothing on any of them
        and pass regardless of where the push went.
        """
        hub = Hub(str(self.repos.state), workdir=self.tmp / "gatehub",
                  code_url=str(self.repos.code),
                  code_push_url=str(self.repos.code),
                  tip_push_url=str(self.repos.tip))
        seen: dict = {}
        real_delete = train._delete_code_ref_cas

        def spy_delete(h, ref, expect_sha):
            if "train-tmp-" in ref and "code" not in seen:
                seen["code"] = self.repos.refs_on(self.repos.code,
                                                  "refs/heads/staging/train-tmp-*")
                seen["tip"] = self.repos.refs_on(self.repos.tip,
                                                 "refs/heads/staging/train-tmp-*")
                seen["state"] = self.repos.refs_on(self.repos.state, "refs/heads/*")
            return real_delete(h, ref, expect_sha)

        with mock.patch.object(train, "_delete_code_ref_cas", spy_delete):
            res = self.run_train(
                gate_fn=lambda clone, label: train.real_gate(clone, label, hub=hub))

        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")
        self.assertTrue(seen, "real_gate never pushed a temp gate ref at all")
        self.assert_routing(res, expect_tmp_refs=seen)
        # ...and it really was cleaned up afterwards, on the repo it went to.
        self.assertEqual(
            self.repos.refs_on(self.repos.code, "refs/heads/staging/train-tmp-*"), {})

    # -- negative controls --------------------------------------------- #

    def test_negative_control_tip_pointed_at_the_code_repo_is_caught(self):
        """The pre-split misconfiguration: one URL for the tip and for the
        three PAT writes. Every ref still lands somewhere plausible and the
        run still reports `advanced` -- which is exactly why this needed a
        test rather than a reviewer. `assert_routing` must REFUSE it.
        """
        res = self.run_train(tip_push_url=self.repos.code)
        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")
        # The run looks entirely healthy from the outside...
        self.assertEqual(res.landed, ["staging/alpha"])
        self.assertEqual(
            self.repos.refs_on(self.repos.code, "refs/heads/*").get(TIP_REF),
            res.new_tip,
            "the tip went to the code repo, which is the misconfiguration under test")
        self.assertNotEqual(
            self.repos.refs_on(self.repos.tip, "refs/heads/*").get(TIP_REF),
            res.new_tip)
        # ...and the routing assertion is what notices.
        with self.assertRaises(AssertionError):
            self.assert_routing(res)

    def test_negative_control_rescued_sent_through_push_tip_ref_is_caught(self):
        """T1's literal wording: "a train that sends rescued/* through
        `push_tip_ref` passes every test". Not any more.

        `Hub.push_code_ref` is replaced by `Hub.push_tip_ref` for the
        duration -- the exact regression, at the exact seam -- and the
        consequences are asserted directly as well as through
        `assert_routing`, because the failure is quiet: the rescue push
        SUCCEEDS (against the wrong repo), the verify-by-sha read then
        finds nothing on the code repo, and the branch is reported as
        "landed but rescue unverified" with its staging ref kept.
        """
        with mock.patch.object(Hub, "push_code_ref", Hub.push_tip_ref):
            res = self.run_train()

        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")
        tip = self.repos.refs_on(self.repos.tip, "refs/heads/*")
        code = self.repos.refs_on(self.repos.code, "refs/heads/*")
        self.assertEqual(tip.get("refs/heads/rescued/alpha"), self.repos.alpha_sha,
                         "the mis-route did not actually happen; the control is vacuous")
        self.assertNotIn("refs/heads/rescued/alpha", code)
        with self.assertRaises(AssertionError):
            self.assert_routing(res)


# --------------------------------------------------------------------- #
# T3 -- a train gate's verdict-store failure reaches the train's own result
# --------------------------------------------------------------------- #


class TestTrainGateSurfacesVerdictStoreFailure(_ThreeRepoCase):
    """`fleetd`'s reap loop reads the verdict-store-failure marker only for
    gates fleetd itself spawned. `train.real_gate` runs `gate.sh` as a
    plain subprocess of the train's own process -- there is no worker, no
    claim and no reap -- so a train gate's marker was read by nothing, on
    any host, ever. The train reads its own now.
    """

    def _gate_hub(self) -> Hub:
        return Hub(str(self.repos.state), workdir=self.tmp / "gatehub",
                   code_url=str(self.repos.code),
                   code_push_url=str(self.repos.code),
                   tip_push_url=str(self.repos.tip))

    def test_a_marker_left_by_the_gate_is_recorded_on_the_run(self):
        os.environ["FIXTURE_GATE_MARK_STORE_FAILED"] = "1"
        hub = self._gate_hub()
        warnings: list = []
        res = self.run_train(
            gate_fn=lambda clone, label: train.real_gate(
                clone, label, hub=hub, warnings=warnings))

        # The verdict itself is untouched by a hub-cache outage: the run
        # still lands. That is the property the marker exists BECAUSE of.
        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")
        self.assertEqual(len(warnings), 1, warnings)
        label, detail = warnings[0]
        self.assertIn("alpha", label)
        self.assertIn("could not push its verdict to the hub cache", detail)

    def test_no_marker_means_no_warning(self):
        """The negative case, and it is not decoration: without it a
        `real_gate` that appended a warning unconditionally would pass the
        test above."""
        hub = self._gate_hub()
        warnings: list = []
        res = self.run_train(
            gate_fn=lambda clone, label: train.real_gate(
                clone, label, hub=hub, warnings=warnings))
        self.assertEqual(res.outcome, "advanced", msg=f"ejected={res.ejected}")
        self.assertEqual(warnings, [])

    def test_the_marker_filename_the_train_reads_is_the_one_gate_sh_writes(self):
        """The stub gate.sh above builds the marker name from
        `config.VERDICT_STORE_FAILED_SUFFIX`, so this test would still pass
        if BOTH sides were renamed together -- which is correct, and is why
        `test_verdict_marker_seam.py` exists to pin that constant against
        the real `gate.sh`'s own `SV=` line. What is checked here is that
        `real_gate` looks in `$HOME/gatelogs`, the directory gate.sh
        actually writes to, rather than in the clone or a temp dir.
        """
        os.environ["FIXTURE_GATE_MARK_STORE_FAILED"] = "1"
        hub = self._gate_hub()
        warnings: list = []
        self.run_train(gate_fn=lambda clone, label: train.real_gate(
            clone, label, hub=hub, warnings=warnings))
        markers = sorted((self._home / "gatelogs").glob(config.verdict_store_failed_glob()))
        self.assertTrue(markers, "the stub gate never wrote a marker")
        self.assertEqual(train.gatelogs_dir(), self._home / "gatelogs")
        # The marker is NOT consumed: fleetd's durable sweep of the same
        # directory must still be able to see it.
        self.assertTrue(markers[0].is_file())


# --------------------------------------------------------------------- #
# T4 -- an ssh tip URL REQUIRES the deploy key
# --------------------------------------------------------------------- #


class TestTipPushCredentialRule(_ThreeRepoCase):
    """With `FLEET_TRAIN_DEPLOY_KEY` absent and an ssh `tip_push_url`, the
    tip push does not fail -- it SUCCEEDS, under whatever key the ambient
    ssh agent offers. Nothing in the run's output distinguishes that from
    the intended credential, and the tip is the one write that bypasses the
    `tip-update` ruleset. So this is a refusal, not a warning.
    """

    def setUp(self):
        super().setUp()
        self.key = self.tmp / "train_deploy_key"
        self.key.write_text("not a real key, only its existence is read\n")

    def test_ssh_tip_url_without_the_deploy_key_is_refused(self):
        with self.assertRaises(train.TrainError) as cm:
            train.check_tip_push_credential("git@github.com:swack-tools/oxidex.git")
        msg = str(cm.exception)
        self.assertIn(train.TRAIN_DEPLOY_KEY_ENV, msg)
        self.assertIn("ambient ssh agent", msg)

    def test_the_ssh_url_scheme_spelling_is_refused_too(self):
        with self.assertRaises(train.TrainError):
            train.check_tip_push_credential("ssh://git@github.com/swack-tools/oxidex.git")

    def test_ssh_tip_url_with_the_deploy_key_is_allowed(self):
        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(self.key)
        train.check_tip_push_credential("git@github.com:swack-tools/oxidex.git")

    def test_a_key_that_names_a_missing_file_is_not_a_key(self):
        """`_train_deploy_key_ssh_command` already warns and returns None
        for a path that does not exist -- which means the variable being
        SET is not evidence the key is usable, and the refusal has to ask
        the resolver rather than the environment."""
        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(self.tmp / "no-such-key")
        with self.assertRaises(train.TrainError):
            train.check_tip_push_credential("git@github.com:swack-tools/oxidex.git")

    def test_an_https_tip_url_with_a_key_set_warns_that_the_key_is_inert(self):
        """The other direction, and only a warning: `GIT_SSH_COMMAND` is
        not consulted for an https remote at all, so the key is inert
        rather than wrong and the push fails loudly on its own if the PAT
        is missing. Worth a line because "I set the deploy key" is
        otherwise indistinguishable from "the deploy key is in use"."""
        import contextlib
        import io

        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(self.key)
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf):
            train.check_tip_push_credential("https://github.com/swack-tools/oxidex.git")
        self.assertIn("INERT", buf.getvalue())
        self.assertIn(train.TRAIN_DEPLOY_KEY_ENV, buf.getvalue())

    def test_a_local_path_is_neither_case_and_is_left_alone(self):
        """Every fixture in this suite, and the single-repo bare-hub
        topology. Nothing about a local path can silently borrow an ssh
        identity, so neither the refusal nor the warning applies -- and if
        either did, this whole file would refuse to run."""
        import contextlib
        import io

        os.environ[train.TRAIN_DEPLOY_KEY_ENV] = str(self.key)
        buf = io.StringIO()
        with contextlib.redirect_stderr(buf):
            train.check_tip_push_credential(str(self.repos.tip))
        self.assertEqual(buf.getvalue(), "")

    def test_run_train_refuses_before_it_claims_the_singleton(self):
        """The check is worth nothing at the push site alone: by then the
        run has held the fleet-wide train singleton and paid 20-45 minutes
        of gate. So `run_train` asks first.

        The instrument is `train.Claim` itself, not the refs left behind
        afterwards: a run that DID claim and then failed at the push
        releases its claim on the way out, so the state repo looks
        identical either way. Asserting on the leftover refs would pass
        against the very ordering this test exists to pin.
        """
        entered: list = []
        real_claim = train.Claim

        def spy_claim(*a, **kw):
            entered.append(kw.get("kind"))
            return real_claim(*a, **kw)

        with mock.patch.object(train, "Claim", spy_claim):
            with self.assertRaises(train.TrainError) as cm:
                self.run_train(tip_push_url="git@github.com:swack-tools/oxidex.git")
        self.assertIn(train.TRAIN_DEPLOY_KEY_ENV, str(cm.exception))
        self.assertEqual(entered, [],
                         "the train claimed the singleton before discovering it "
                         "could not push the tip under a chosen identity")

    def test_a_dry_run_is_exempt_because_it_writes_nothing(self):
        """`--dry-run` is the "what would this configuration do" probe, and
        refusing it would make the one command that could have TOLD an
        operator about the missing key refuse to answer."""
        res = self.run_train(tip_push_url="git@github.com:swack-tools/oxidex.git",
                             dry_run=True)
        self.assertEqual(res.outcome, "empty")

    def test_push_tip_re_asks_at_the_write_itself(self):
        """Defence in depth: `run_train` is not the only way to reach the
        push (`_push_tip` is called directly by tests and by any future
        caller), so the guard is also on the write.

        The check is stubbed to raise rather than allowed to run for real:
        what is under test is that `_push_tip` ASKS, and asks about its own
        hub's `tip_push_url`, before it does anything else. Letting the
        real check run would leave the assertion satisfiable by a
        `_push_tip` that never called it and merely failed later for an
        unrelated reason -- which is how the first draft of this test
        passed against the bug.
        """
        hub = Hub(str(self.repos.state), workdir=self.tmp / "guard",
                  code_url=str(self.repos.code),
                  tip_push_url="git@github.com:swack-tools/oxidex.git")
        sentinel = train.TrainError("stubbed credential refusal")
        with mock.patch.object(train, "check_tip_push_credential",
                               side_effect=sentinel) as spy:
            # `Exception`, not `TrainError`: a `_push_tip` that skipped the
            # check would carry on into a real push and fail some other
            # way, and the point is that the SPY was called -- asserted
            # below -- not merely that something went wrong.
            with self.assertRaises(Exception) as cm:  # noqa: B017
                train._push_tip(hub, self.tmp, [])
        spy.assert_called_once_with("git@github.com:swack-tools/oxidex.git")
        self.assertIs(cm.exception, sentinel)


if __name__ == "__main__":
    unittest.main()
