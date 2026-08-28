#!/usr/bin/env python3
"""Bring-up simulation of the split spine (docs/AGENT-SERVER-SPEC.md §3.1,
§4.4; docs/AGENT-SERVER-PLAN.md Stage 1): a REAL `fleetd.py` process,
started exactly the way the units start it on a host --
`fleetd --hub <state> --code <code>` -- against two LOCAL bare repos that
play the PRIVATE state repo and the PUBLIC code repo.

What the Stage 1 integration review found was not one bug but a family:
each module individually believed it had been taught about the split
while the daemon as a whole still died, idled, or wrote to the wrong
repo (B1 tip reads from the state repo, B4 gate children spawned without
FLEET_HUB_URL/FLEET_CODE_URL, S1 an enabled 0/0 host with an empty
`refused`, S2 agents cloning the state repo). Unit tests on each module
passed throughout, because every one of them ran the fixture with
`code_url == url`. So this file does what none of them did: it seeds
`refs/fleet/desired` with the real `rollout/seed_desired.py`, raises one
host's target with the real `cli.py up`, runs the real daemon for several
reconcile loops against a state repo that carries NO `refs/heads/*` and a
code repo that carries NO `refs/fleet/*`, and then asks the two repos --
not the daemon -- what happened.

Assertions (each is one of the review's findings turned into a check):
  * the host singleton AND a gate claim appeared on STATE, never on CODE;
  * the heartbeat on STATE carries a `refused` reason or a started gate
    (`gates_running >= 1`) -- never the silent `[]` with nothing started;
  * the gate child's environment carried FLEET_HUB_URL == state and
    FLEET_CODE_URL == code (B4), so a real `gate.sh` would not ABORT;
  * no ref was written to CODE at all -- only the train writes code refs,
    and no train ran here -- and STATE never grew a `refs/heads/*`;
  * the daemon never died: it wrote >= 3 reconcile lines and exited 0 on
    SIGTERM, releasing its singleton.

The gate is a stub (FLEET_PLAN.md: "mock the gate") that records its
environment, writes PASS, and parks until released so its claim is
observable. Nothing here touches the real hub, the real code repo, or the
real `~/.fleetd` (HOME is redirected into the fixture).

Instrument: plain `unittest`, standard library only, against throwaway
`git init --bare` repos under `tempfile.gettempdir()`.

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_bringup_split -v
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import claim as claim_mod  # noqa: E402
from fleetlib import Hub  # noqa: E402
from _env import HermeticCase, scrub_env  # noqa: E402

TIP_REF = "refs/heads/refactor/tag-machinery"
STAGING_REF = "refs/heads/staging/x"
HOST = "server"  # a host `rollout/seed_desired.py` actually seeds
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}

# The daemon-backed timescale from test_lease_protocol: absolute margins,
# not ratios, because a loaded host eats the fast in-process ones.
DTTL_S = 12.0
DRENEW_S = 2.0


def _run(args, cwd=None, check=True, env=None):
    return subprocess.run(
        args, cwd=cwd, check=check, capture_output=True, text=True,
        env=scrub_env(**{**GIT_ENV, **(env or {})}),  # `env` may carry its own GIT_* (see _env())
    )


def _ls_remote(repo: Path) -> dict:
    """{refname: sha} for every ref on a bare repo -- the instrument every
    'what was written where' assertion below reads from. `--refs`: no
    peeled `^{}` lines."""
    out = _run(["git", "ls-remote", "--refs", str(repo)]).stdout
    refs = {}
    for line in out.splitlines():
        sha, _, name = line.partition("\t")
        if name:
            refs[name] = sha
    return refs


class TestBringupSplitSpine(HermeticCase):
    def setUp(self):
        super().setUp()
        self.tmpdir = tempfile.TemporaryDirectory(prefix="bringup-split-")
        self.tmp = Path(self.tmpdir.name)
        assert str(self.tmp).startswith(tempfile.gettempdir())
        self.state = self.tmp / "state.git"
        self.code = self.tmp / "code.git"
        self.home = self.tmp / "home"
        (self.home / ".fleetd").mkdir(parents=True)
        self.log_dir = self.tmp / "gatelogs"
        self.gate_env_dir = self.tmp / "gate-env"
        self.gate_env_dir.mkdir()
        self.verdict_dir = self.tmp / "verdicts"
        self.verdict_dir.mkdir()
        self._seed_repos()
        self.stub = self._make_stub_gate()
        self.daemon = None
        self.daemon_log_path = self.tmp / "fleetd.log"

    def tearDown(self):
        # Release any parked stub gate, then make sure nothing we spawned
        # outlives the test: signal OUR daemon's own process group by pid
        # (start_new_session -> pgid == pid). Never a pattern match.
        (self.tmp / "stop-all").write_text("")
        if self.daemon is not None and self.daemon.poll() is None:
            try:
                os.killpg(self.daemon.pid, signal.SIGTERM)
                self.daemon.wait(timeout=30)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                try:
                    os.killpg(self.daemon.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        deadline = time.time() + 30
        while time.time() < deadline:
            leftovers = _ls_remote(self.state)
            if not any(r.startswith("refs/fleet/claims/gate/") for r in leftovers):
                break
            time.sleep(0.5)
        self.tmpdir.cleanup()

    # ------------------------------------------------------------------ #
    # Fixture
    # ------------------------------------------------------------------ #

    def _seed_repos(self):
        """state.git: EMPTY. code.git: a tip plus one staging/x branch with a
        real commit past it (so the queue sees a non-ancestor)."""
        _run(["git", "init", "-q", "--bare", str(self.state)])
        _run(["git", "init", "-q", "--bare", str(self.code)])
        work = self.tmp / "seed"
        _run(["git", "init", "-q", str(work)])
        (work / "f.txt").write_text("tip\n")
        _run(["git", "-C", str(work), "add", "."])
        _run(["git", "-C", str(work), "commit", "-qm", "tip"])
        _run(["git", "-C", str(work), "push", "-q", str(self.code), f"HEAD:{TIP_REF}"])
        self.tip_sha = _run(["git", "-C", str(work), "rev-parse", "HEAD"]).stdout.strip()
        (work / "g.txt").write_text("branch\n")
        _run(["git", "-C", str(work), "add", "."])
        _run(["git", "-C", str(work), "commit", "-qm", "branch work"])
        _run(["git", "-C", str(work), "push", "-q", str(self.code), f"HEAD:{STAGING_REF}"])
        self.staging_sha = _run(["git", "-C", str(work), "rev-parse", "HEAD"]).stdout.strip()

    def _make_stub_gate(self) -> Path:
        """Installed at `<repo-root>/tools/fleet/gate.sh` under a throwaway
        repo root, which is exactly where `fleetd.default_gate_command`
        looks -- so the daemon is started with NO test-only gate hook, just
        `--repo-root`. Records the env it was spawned with (the B4 check),
        writes PASS, parks until released."""
        root = self.tmp / "root"
        (root / "tools" / "fleet").mkdir(parents=True)
        (root / "tools" / "fleet" / "gate_version.txt").write_text("stub-7\n")
        stub = root / "tools" / "fleet" / "gate.sh"
        stub.write_text(
            "#!/bin/bash\n"
            "# $1=branch $2=tag $3=scope-token (inert)\n"
            f"printf 'FLEET_HUB_URL=%s\\nFLEET_CODE_URL=%s\\nBRANCH=%s\\n' "
            f"\"${{FLEET_HUB_URL-<unset>}}\" \"${{FLEET_CODE_URL-<unset>}}\" \"$1\" "
            f"> '{self.gate_env_dir}/env-'\"$2\"\n"
            f"printf 'PASS\\n' > '{self.verdict_dir}/gate-'\"$2\"'.verdict'\n"
            f"STOP='{self.tmp}/stop-all'\n"
            "n=0\n"
            'while [ ! -f "$STOP" ] && [ $n -lt 300 ]; do sleep 0.2; n=$((n+1)); done\n'
            "exit 0\n"
        )
        stub.chmod(0o755)
        self.repo_root = root
        return stub

    def _env(self) -> dict:
        """What a unit file gives fleetd -- minus FLEET_HUB_URL/FLEET_CODE_URL,
        deliberately: the repos arrive on ARGV only, which is the B4 shape
        (argv config never reached spawned gates). `scrub_env` drops those
        two along with every other FLEET_*/KEEL_* the invoker exported."""
        env = scrub_env()
        env.update({
            **GIT_ENV,
            "HOME": str(self.home),  # hubcache/seedcache/clicache live here, never ~
            "FLEET_HOST": HOST,
            # The orphan sweep's marker: only OUR stub, so the sweep never
            # even looks at a real gate on this machine.
            "FLEET_WORKER_MARKERS": str(self.stub),
            claim_mod.TTL_ENV: str(DTTL_S),
            claim_mod.RENEW_ENV: str(DRENEW_S),
        })
        return env

    def _zero_limits(self):
        """The daemon under test is REAL, so `disk_probe`/`mem_probe` measure
        this machine -- and a laptop with 6G free would refuse the gate
        with `limits` (correct, but not what this test measures). The
        operator's equivalent of `fleet up` for the floors is a CAS edit of
        `desired.limits`, done the way `cli._edit_desired` does it."""
        hub = Hub(str(self.state), workdir=self.tmp / "limits-cache")
        ref = "refs/fleet/desired"
        cur = hub.sha(ref)
        doc = hub.read(ref)
        doc["limits"] = {"min_free_gb": 0, "min_free_mem_gb": 0}
        doc["generation"] = int(doc.get("generation", 0)) + 1
        self.assertTrue(hub.update(ref, doc, cur))

    def _wait(self, pred, timeout: float, what: str):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.daemon is not None and self.daemon.poll() is not None:
                self.fail(f"fleetd exited rc={self.daemon.returncode} while waiting for "
                          f"{what}\n--- fleetd.log ---\n{self._daemon_log()}")
            v = pred()
            if v:
                return v
            time.sleep(0.5)
        self.fail(f"timed out after {timeout}s waiting for {what}\n--- fleetd.log ---\n"
                  f"{self._daemon_log()}")

    def _daemon_log(self) -> str:
        try:
            return self.daemon_log_path.read_text(errors="replace")
        except OSError:
            return "(no log)"

    def _reconcile_lines(self) -> list:
        return [ln for ln in self._daemon_log().splitlines()
                if ln.startswith(f"fleetd[{HOST}] gates=")]

    # ------------------------------------------------------------------ #
    # The simulation
    # ------------------------------------------------------------------ #

    def test_bringup_against_split_state_and_code_repos(self):
        env = self._env()
        code_before = _ls_remote(self.code)
        self.assertEqual(set(code_before), {TIP_REF, STAGING_REF})
        self.assertEqual(_ls_remote(self.state), {}, "state repo starts empty")

        # 1. seed_desired against STATE -- the real rollout script.
        seed = _run([sys.executable, str(FLEET_DIR / "rollout" / "seed_desired.py"),
                     "--hub", str(self.state), "--execute"], env=env)
        self.assertIn("created refs/fleet/desired: True", seed.stdout, seed.stderr)
        # 2. raise the host's target with the real CLI (`fleet up`).
        up = _run([sys.executable, str(FLEET_DIR / "cli.py"), "--hub", str(self.state),
                   "up", HOST, "--gates", "1"], env=env)
        self.assertIn(f"{HOST} -> ", up.stdout, up.stderr)
        self._zero_limits()
        state_after_seed = _ls_remote(self.state)
        self.assertEqual(set(state_after_seed), {"refs/fleet/desired"})
        self.assertEqual(_ls_remote(self.code), code_before,
                         "seeding desired must not touch the code repo")

        # 3. the real daemon, argv-configured: --hub <state> --code <code>.
        daemon_log = open(self.daemon_log_path, "wb")
        self.addCleanup(daemon_log.close)
        self.daemon = subprocess.Popen(
            [sys.executable, str(FLEET_DIR / "fleetd.py"),
             "--hub", str(self.state), "--code", str(self.code),
             "--interval", "1",
             "--repo-root", str(self.repo_root),
             "--log-dir", str(self.log_dir)],
            stdout=daemon_log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
            env=env, start_new_session=True,
        )

        # -- a claim appeared on STATE: first the host singleton ...
        singleton_ref = f"refs/fleet/claims/host/{HOST}"
        self._wait(lambda: singleton_ref in _ls_remote(self.state), 60,
                   "the host singleton claim on the STATE repo")
        # ... then the gate's own claim, once the stub gate is spawned.
        gate_claims = self._wait(
            lambda: [r for r in _ls_remote(self.state) if r.startswith("refs/fleet/claims/gate/")],
            90, "a gate claim on the STATE repo")
        self.assertEqual(len(gate_claims), 1, gate_claims)

        # -- the gate child's env carried both URLs (B4): read what the stub
        # itself saw, not what fleetd says it passed.
        env_files = self._wait(lambda: list(self.gate_env_dir.glob("env-*")), 30,
                               "the stub gate recording its environment")
        seen = dict(line.split("=", 1) for line in env_files[0].read_text().splitlines())
        self.assertEqual(seen["FLEET_HUB_URL"], str(self.state), seen)
        self.assertEqual(seen["FLEET_CODE_URL"], str(self.code), seen)
        self.assertEqual(seen["BRANCH"], "staging/x", seen)
        verdicts = list(self.verdict_dir.glob("gate-*.verdict"))
        self.assertEqual(len(verdicts), 1)
        self.assertEqual(verdicts[0].read_text().strip(), "PASS")

        # -- ~3 reconciles, with the gate parked the whole time.
        self._wait(lambda: len(self._reconcile_lines()) >= 3, 60,
                   "three reconcile lines from the daemon")

        # -- the heartbeat on STATE: a started gate or refused reasons,
        # never the silent empty case.
        hub = Hub(str(self.state), workdir=self.tmp / "observer-cache", code_url=str(self.code))
        hb = hub.read(f"refs/fleet/hosts/{HOST}")
        self.assertIsNotNone(hb, "no heartbeat on the state repo")
        refused = hb.get("refused")
        self.assertIsInstance(refused, list)
        self.assertTrue(
            hb.get("gates_running", 0) >= 1 or refused,
            f"heartbeat is silent: gates_running={hb.get('gates_running')} refused={refused}",
        )
        self.assertEqual(hb.get("gates_running"), 1, hb)
        self.assertEqual(hb.get("gate_version"), "stub-7", hb)
        # And the reconcile line says the gate started -- not a refusal
        # that happens to look alive.
        first = self._reconcile_lines()[0]
        self.assertIn("started=['", first, first)
        self.assertNotIn("queue-unavailable", self._daemon_log(),
                         "the tip was read from the state repo (B1)")
        self.assertNotIn("RECONCILE DEGRADED", self._daemon_log())
        self.assertNotIn("Traceback", self._daemon_log())

        # -- no ref was ever written to CODE (only the train does that, and
        # no train ran); STATE never grew a refs/heads/*.
        self.assertEqual(_ls_remote(self.code), code_before,
                         "the daemon wrote to the CODE repo")
        state_refs = _ls_remote(self.state)
        self.assertFalse([r for r in state_refs if r.startswith("refs/heads/")],
                         f"code refs leaked onto the STATE repo: {state_refs}")
        self.assertTrue(all(r.startswith("refs/fleet/") for r in state_refs), state_refs)
        self.assertIn("refs/fleet/desired", state_refs)
        self.assertIn(singleton_ref, state_refs)
        self.assertIn(f"refs/fleet/hosts/{HOST}", state_refs)

        # -- drain the host with the real CLI (`fleet drain`: converge to
        # zero WITHOUT killing): the next heartbeat must still show the
        # gate running AND now carry the S1 `target-zero` reason -- an
        # enabled 0/0 host says why it starts nothing, while the live gate
        # it already has is left alone. This is also what keeps the
        # release below from churning: with the target still at 1 the
        # daemon would re-offer `staging/x` (the stub stores no verdict)
        # the moment its claim is freed, one new gate per loop.
        drain = _run([sys.executable, str(FLEET_DIR / "cli.py"), "--hub", str(self.state),
                      "drain", HOST], env=env)
        self.assertIn(HOST, drain.stdout, drain.stderr)
        n_before = len(self._reconcile_lines())
        self._wait(lambda: len(self._reconcile_lines()) >= n_before + 2, 60,
                   "two reconciles after the drain")
        hb2 = hub.read(f"refs/fleet/hosts/{HOST}")
        self.assertEqual(hb2.get("gates_running"), 1, "drain must never kill live work")
        self.assertIn("target-zero", [r[0] for r in hb2.get("refused") or []], hb2)
        self.assertEqual(_ls_remote(self.code), code_before)

        # -- release the gate; the daemon reaps it, releases the claim, and
        # keeps going (the PASS it wrote is the gate's business, not ours).
        (self.tmp / "stop-all").write_text("")
        self._wait(
            lambda: not [r for r in _ls_remote(self.state) if r.startswith("refs/fleet/claims/gate/")],
            60, "the finished gate's claim to be released")
        self._wait(lambda: "finished=['" in self._daemon_log(), 30,
                   "the daemon reporting the finished gate")
        self.assertEqual(len(list(self.gate_env_dir.glob("env-*"))), 1,
                         "exactly one gate was ever spawned")
        self.assertEqual(_ls_remote(self.code), code_before)

        # -- clean stop: SIGTERM -> rc 0, singleton released, still no code
        # writes.
        self.daemon.send_signal(signal.SIGTERM)
        rc = self.daemon.wait(timeout=60)
        self.assertEqual(rc, 0, self._daemon_log())
        self.assertNotIn(singleton_ref, _ls_remote(self.state),
                         "a cleanly stopped fleetd must release its host singleton")
        self.assertEqual(_ls_remote(self.code), code_before,
                         "the daemon wrote to the CODE repo on the way out")
        # Final heartbeat still on STATE, and it still names the split hub.
        self.assertIn(f"refs/fleet/hosts/{HOST}", _ls_remote(self.state))

    def test_enabled_zero_target_host_reports_why_on_state(self):
        """S1 on the real spine: the seeded desired state leaves every host
        at 0/0 enabled. One `--once` reconcile must land a heartbeat on
        STATE whose `refused` says so, and `fleet status --why` must render
        it -- the exact line PLAN Stage 1's acceptance asks for instead of
        '(no refused reasons on file)'."""
        env = self._env()
        seed = _run([sys.executable, str(FLEET_DIR / "rollout" / "seed_desired.py"),
                     "--hub", str(self.state), "--execute"], env=env)
        self.assertIn("created refs/fleet/desired: True", seed.stdout, seed.stderr)
        code_before = _ls_remote(self.code)

        res = _run([sys.executable, str(FLEET_DIR / "fleetd.py"),
                    "--hub", str(self.state), "--code", str(self.code), "--once",
                    "--repo-root", str(self.repo_root), "--log-dir", str(self.log_dir)],
                   env=env, check=False)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertNotIn("Traceback", res.stdout + res.stderr)

        hub = Hub(str(self.state), workdir=self.tmp / "observer-cache", code_url=str(self.code))
        hb = hub.read(f"refs/fleet/hosts/{HOST}")
        self.assertIsNotNone(hb, "no heartbeat on the state repo")
        reasons = [r[0] for r in hb.get("refused") or []]
        self.assertIn("target-zero", reasons, hb)
        self.assertEqual(hb.get("gates_running"), 0)
        self.assertEqual(_ls_remote(self.code), code_before, "a 0/0 host wrote to CODE")
        self.assertFalse([r for r in _ls_remote(self.state) if r.startswith("refs/heads/")])

        why = _run([sys.executable, str(FLEET_DIR / "cli.py"), "--hub", str(self.state),
                    "status", "--why"], env=env)
        self.assertIn("refused: target-zero (gates 0 / agents 0)", why.stdout, why.stdout)


if __name__ == "__main__":
    unittest.main()
