#!/usr/bin/env python3
"""Tests for tools/fleet/keel/cli.py -- PLAN Stage 2 task 6.

    'keel status|events|server {status,rehost}' over
    FallbackHub(ServerHub, GitHub/bare Hub) with --direct bypassing the
    server entirely; --json output; 'keel events --follow' consuming SSE;
    config from env (FLEET_HUB_URL, FLEET_CODE_URL, KEEL_SERVER_URL,
    KEEL_TOKEN_FILE).

Every test invokes the REAL `tools/fleet/keel/cli.py` file as a
subprocess (`sys.executable <cli.py> ...`), never an in-process import --
`cli.py`'s own basename collides with the pre-existing
`tools/fleet/cli.py`, which several other `test_*.py` modules already
import unqualified as `import cli`; a subprocess gets its own fresh
`sys.modules` and sidesteps that entirely (see `keel/cli.py`'s and
`keel/serverhub.py`'s module docstrings for the full account of why an
in-process `import cli` here would be unsafe run alongside them in one
`python3 -m unittest` invocation, which the project's own test-running
convention does).

Fixture (`_KeelCliCase`, HermeticCase-based per Stage 1e's fence): two
throwaway `git init --bare` repos under the system temp dir (state +
code, asserted under it), a tip commit pushed to the code repo, and a
real `KeelHTTPServer` on `127.0.0.1:0` fronting a real `CachedHub`/
`CachedHubStore` (`keel/hubstore.py`) over the STATE repo, with a hashed
bearer token. `env()` builds the hermetic subprocess environment
(`FLEET_HUB_URL`, `FLEET_CODE_URL`, `KEEL_SERVER_URL`, `KEEL_TOKEN_FILE`)
every command below runs under; `run_cli()` invokes the CLI as that
subprocess.

What is pinned, and the bug that makes each test fail (checked by
reverting the relevant fix in a scratch copy of `keel/cli.py`/
`keel/serverhub.py` and re-running just that test -- noted per test):
  * `keel status` (via the server) and `keel status --direct` agree on
    every field except the ones that are inherently about HOW/WHEN this
    one invocation answered (`ts`, `server`, and each host's
    `heartbeat_age_s`, which is `now() - hb['ts']` and so differs between
    ANY two invocations seconds apart, route notwithstanding) -- SPEC
    SS3.4's own re-host acceptance instrument
    (`diff ... | jq 'del(.ts,.server)'`) generalized to this JSON shape.
    Checked: `compute_status` itself is one function shared by both
    routes, so a bug planted THERE breaks both sides identically and an
    agreement check cannot see it (tried it: hardcoding `hosts = []`
    leaves the two payloads equal, uselessly). The property this test
    actually pins is that the SERVER ROUTE reproduces what `compute_status`
    sees over a raw hub -- so the meaningful fault injection is
    route-specific: making `ServerHub.list()` return `{}` unconditionally
    (dropping every entry the server actually reported) makes
    `test_status_server_and_direct_agree` fail on the `hosts` and `queue`
    keys with a real assertion diff, not an error.
  * `--direct` never constructs a `ServerHub` / never touches
    `KEEL_SERVER_URL` at all -- pinned with `KEEL_SERVER_URL` set to a
    string `ServerHub.__init__` rejects outright (`ValueError`, no
    scheme/netloc), not merely an unreachable address: a dead-but-well-
    formed URL is the WEAKER check and does not actually pin this
    (tried it: `FallbackHub`'s own read fallback silently masks a
    `--direct` that still built `FallbackHub(ServerHub(...), github)`,
    since every read falls through to `github` regardless). Checked:
    deleting the `if getattr(args, "direct", False): return github, None`
    early return in `build_hub` makes this test fail with an uncaught
    `ValueError` from `ServerHub.__init__` (rc 1, traceback in stderr),
    not a clean `--direct` answer.
  * `keel events --follow`, started as a background process against the
    live fixture server, receives a write appended to the event ring
    AFTER the process was already started and connected. Checked:
    breaking `_parse_sse_frame`'s `id:` field match (so `seq` never
    parses) makes every frame look field-less and get silently dropped;
    `--follow`'s idle timeout then fires with nothing ever printed, and
    the test fails on a `None` line rather than hanging forever.
  * config precedence: `--hub`/`--server`/`--token-file` flags win over
    `FLEET_HUB_URL`/`KEEL_SERVER_URL`/`KEEL_TOKEN_FILE`, and the env vars
    alone (no flags) are sufficient. Checked: swapping `_resolve`'s
    `getattr(args, attr, None) or os.environ.get(env)` to just
    `os.environ.get(env)` makes the flag-override test fail (the
    deliberately-wrong env value wins).
  * `keel server rehost` refuses (exit 3) against a live lease and
    succeeds (exit 0, CAS-visible on the hub) against an absent or
    EXPIRED one. Checked: dropping the `is_expired(cur_payload)` guard
    (treating any present payload as live) makes
    `test_server_rehost_reacquires_an_expired_lease` fail with exit 3
    instead of 0.

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_keel_cli -v
"""

from __future__ import annotations

import hashlib
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Dict, Optional

FLEET_DIR = Path(__file__).resolve().parents[1]
KEEL_DIR = FLEET_DIR / "keel"
for _p in (FLEET_DIR, KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

# `server`/`store_api`/`hubstore` are imported bare everywhere in this
# suite (test_server_transport.py, test_hubstore.py) -- matched here.
# `cachedhub`/`fallbackhub` are imported qualified (`keel.<name>`)
# everywhere -- also matched. `keel/cli.py` itself is NEVER imported
# in-process (see module docstring): only ever invoked as a subprocess.
import hubstore  # noqa: E402
import server as keel_server  # noqa: E402
from _env import HermeticCase  # noqa: E402
from claim import claim_ref, is_expired  # noqa: E402
from claim import _iso as claim_iso  # noqa: E402 -- the ONE lease spelling
from fleetlib import Hub  # noqa: E402
from keel.cachedhub import CachedHub  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402

CLI_PATH = KEEL_DIR / "cli.py"
TIP_REF = "refactor/tag-machinery"
SERVER_CLAIM_REF = claim_ref("server", "singleton")


def _bare_repo(root: Path, name: str) -> str:
    path = root / name
    result = subprocess.run(["git", "init", "--quiet", "--bare", str(path)], capture_output=True)
    assert result.returncode == 0, result.stderr.decode()
    resolved = str(path.resolve())
    system_tmp = str(Path(tempfile.gettempdir()).resolve())
    assert resolved.startswith(system_tmp), f"fixture {resolved!r} is not under {system_tmp!r}"
    return str(path)


class _KeelCliCase(HermeticCase):
    """Two bare repos (state + code, code seeded with a tip commit) and a
    real `KeelHTTPServer` fronting a real `CachedHub`/`CachedHubStore`
    over the state repo. `self.env()` is the hermetic subprocess
    environment every `run_cli()` call uses by default.
    """

    def setUp(self) -> None:
        super().setUp()
        self._root = Path(tempfile.mkdtemp(prefix="keelcli-test-"))
        # `keel/cli.py`'s `build_github_hub` defaults its git object cache
        # to `Path.home()/".keel"/"clicache"` when no `--hub` flag exists
        # to parameterize it (there isn't one -- it's not part of PLAN
        # Stage 2 task 6's config surface). `Path.home()` reads `HOME`, so
        # redirecting it into the fixture is what keeps every `run_cli()`
        # subprocess below from writing a real `~/.keel/clicache` on
        # whatever machine runs this suite -- the same convention
        # `test_cli.py` already uses for the old `cli.py`'s
        # `~/.fleetd/clicache` (module docstring, HOME redirection).
        os.environ["HOME"] = str(self._root)
        self.state_url = _bare_repo(self._root, "state.git")
        self.code_url = _bare_repo(self._root, "code.git")
        self._seed_tip()

        self.token = "test-token-" + uuid.uuid4().hex[:8]
        tokens = keel_server.TokenStore(
            [{"id": "operator-1", "sha256": hashlib.sha256(self.token.encode()).hexdigest(), "role": "operator"}]
        )
        self.events = keel_server.EventLog(":memory:")
        server_hub = Hub(self.state_url, workdir=self._root / "servercache", code_url=self.code_url)
        self.cached = CachedHub(server_hub)
        self.store = hubstore.CachedHubStore(self.cached)
        config = keel_server.ServerConfig(bind_host="127.0.0.1", port=0)
        self.server = keel_server.build_server(config, store=self.store, tokens=tokens, events=self.events)
        self.server.start()
        _addr, port = self.server.server_address
        self.server_url = f"http://127.0.0.1:{port}"

        self.token_file = self._root / "token.txt"
        self.token_file.write_text(self.token)

        # A second, independent Hub for the test's own direct writes
        # (seeding `desired`/heartbeats/claims) -- never through the
        # thing under test, so a status/rehost bug can't also corrupt
        # the seed.
        self.direct_hub = Hub(self.state_url, workdir=self._root / "directcache", code_url=self.code_url)

        self.addCleanup(self.server.stop)
        self.addCleanup(self.store.close)
        self.addCleanup(lambda: shutil.rmtree(self._root, ignore_errors=True))

    # -- fixture helpers -------------------------------------------------- #

    def _git(self, *args: str, cwd: Optional[Path] = None) -> subprocess.CompletedProcess:
        result = subprocess.run(
            ["git", *args], cwd=str(cwd) if cwd else None,
            env=self.hermetic_env(), capture_output=True, text=True,
        )
        assert result.returncode == 0, f"git {args}: {result.stderr}"
        return result

    def _seed_tip(self) -> None:
        work = self._root / "seed-work"
        self._git("init", "--quiet", str(work))
        (work / "file.txt").write_text("hello\n")
        self._git("add", "file.txt", cwd=work)
        self._git("commit", "--quiet", "-m", "init", cwd=work)
        self._git("branch", "-M", TIP_REF, cwd=work)
        self._git("push", "--quiet", self.code_url, f"{TIP_REF}:{TIP_REF}", cwd=work)

    def push_staging_branch(self, slug: str) -> str:
        """Pushes `refs/heads/staging/<slug>` (one file, one commit) to
        the code repo; returns its sha."""
        work = self._root / f"staging-work-{slug}"
        self._git("clone", "--quiet", self.code_url, str(work))
        (work / f"{slug}.txt").write_text(slug)
        self._git("add", f"{slug}.txt", cwd=work)
        self._git("commit", "--quiet", "-m", slug, cwd=work)
        self._git("checkout", "--quiet", "-b", f"staging/{slug}", cwd=work)
        self._git("push", "--quiet", self.code_url, f"staging/{slug}:staging/{slug}", cwd=work)
        sha = self._git("rev-parse", "HEAD", cwd=work).stdout.strip()
        return sha

    def seed_desired(self, hosts: dict) -> None:
        # Through `self.cached` (the SAME `CachedHub` the fixture server's
        # store wraps), not `self.direct_hub` -- a write-through updates
        # the server's index immediately (`cachedhub.py`'s rule 1); a
        # direct git push the cache never saw is, correctly, invisible to
        # it until the next sweep (no sweeper runs in this fixture, and
        # none is wired into `server.py` yet at Stage 2 -- that is
        # `election.py`/the runner's job, not this CLI's). Seeding this
        # way is what makes "status via server and --direct agree" a
        # meaningful comparison rather than an artifact of an unswept
        # cache.
        ok = self.cached.create("refs/fleet/desired", {"generation": 1, "hosts": hosts})
        assert ok, "seed_desired: refs/fleet/desired already existed"

    def seed_heartbeat(self, host: str, **fields) -> None:
        now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        payload = {"ts": now, "gates_running": 0, "agents_running": 0, "free_gb": 100, "oracle_ok": True}
        payload.update(fields)
        ok = self.cached.create(f"refs/fleet/hosts/{host}", payload)
        assert ok, f"seed_heartbeat: refs/fleet/hosts/{host} already existed"

    def write_server_claim(self, *, expires_in_s: float, holder: str = "some-other-host") -> None:
        """Directly plants `refs/fleet/claims/server/singleton` (bypassing
        `keel server rehost`, which is the thing under test) -- negative
        or positive, per `expires_in_s`'s sign."""
        now = datetime.now(timezone.utc)
        payload = {
            "holder_host": holder,
            "pid": 999999,
            "started_at": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "expires_at": (now + timedelta(seconds=expires_in_s)).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "advertise_urls": [],
            "boot_id": uuid.uuid4().hex,
            "keel_version": "test",
        }
        self.direct_hub.create(SERVER_CLAIM_REF, payload)

    def env(self, **extra: str) -> Dict[str, str]:
        base = self.hermetic_env(
            FLEET_HUB_URL=self.state_url,
            FLEET_CODE_URL=self.code_url,
            KEEL_SERVER_URL=self.server_url,
            KEEL_TOKEN_FILE=str(self.token_file),
        )
        base.update(extra)
        return base

    def run_cli(self, *args: str, env: Optional[Dict[str, str]] = None, timeout: float = 20.0):
        cmd = [sys.executable, str(CLI_PATH), *args]
        return subprocess.run(cmd, env=env if env is not None else self.env(), capture_output=True, text=True, timeout=timeout)

    def popen_cli(self, *args: str, env: Optional[Dict[str, str]] = None) -> subprocess.Popen:
        cmd = [sys.executable, str(CLI_PATH), *args]
        return subprocess.Popen(
            cmd, env=env if env is not None else self.env(),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1,
        )


def _read_one_line(proc: subprocess.Popen, timeout: float) -> Optional[str]:
    """One line from `proc.stdout`, off a background thread, bounded by
    `timeout` -- a blocking `readline()` on the main thread would hang
    the whole suite if the CLI never prints anything."""
    q: "queue.Queue[Optional[str]]" = queue.Queue()

    def reader():
        q.put(proc.stdout.readline())

    t = threading.Thread(target=reader, daemon=True)
    t.start()
    try:
        line = q.get(timeout=timeout)
    except queue.Empty:
        return None
    return line or None


def _strip_time_relative(payload: dict) -> dict:
    """A copy of a `keel status --json` payload with every field that is
    about WHEN/HOW this one invocation ran -- not about the fleet's
    state -- removed: `ts`, `server` (SPEC SS3.4's own instrument), and
    each host's `heartbeat_age_s` (`now() - hb['ts']`, which is `time.
    time()`-relative and so differs between literally any two
    invocations seconds apart, same route or not)."""
    out = json.loads(json.dumps(payload))  # deep copy
    out.pop("ts", None)
    out.pop("server", None)
    for host in out.get("hosts", []):
        host.pop("heartbeat_age_s", None)
    return out


# ------------------------------------------------------------------------ #
# keel status
# ------------------------------------------------------------------------ #


class TestStatus(_KeelCliCase):
    def test_direct_status_reflects_seeded_state(self):
        self.seed_desired({"m5": {"gates": 2, "agents": 1, "enabled": True}})
        self.seed_heartbeat("m5", gates_running=1, oracle_ok=True, owning_user="swack")
        self.push_staging_branch("demo")

        result = self.run_cli("status", "--direct", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)

        self.assertEqual(payload["server"], {"route": "direct"})
        self.assertEqual(payload["desired"]["generation"], 1)
        self.assertEqual(len(payload["hosts"]), 1)
        host = payload["hosts"][0]
        self.assertEqual(host["host"], "m5")
        self.assertEqual(host["state"], "up")
        self.assertEqual(host["gates_running"], 1)
        self.assertEqual(host["gates_wanted"], 2)
        self.assertEqual(host["owning_user"], "swack")
        self.assertEqual(payload["queue"]["slugs"], ["demo"])
        self.assertEqual(payload["queue"]["count"], 1)

    def test_status_server_and_direct_agree(self):
        """The required acceptance test: 'status via server and --direct
        agree (minus ts/server)'."""
        self.seed_desired({"m5": {"gates": 2, "agents": 1, "enabled": True}, "i7": {"gates": 0, "agents": 0, "enabled": False, "reason": "quarantined"}})
        self.seed_heartbeat("m5", gates_running=1, agents_running=1, free_gb=222, oracle_ok=True, owning_user="swack")
        self.seed_heartbeat("i7", gates_running=0)
        self.push_staging_branch("alpha")
        self.push_staging_branch("beta")

        via_server = self.run_cli("status", "--json")
        self.assertEqual(via_server.returncode, 0, via_server.stderr)
        via_direct = self.run_cli("status", "--direct", "--json")
        self.assertEqual(via_direct.returncode, 0, via_direct.stderr)

        server_payload = json.loads(via_server.stdout)
        direct_payload = json.loads(via_direct.stdout)

        self.assertEqual(server_payload["server"]["route"], "server")
        self.assertEqual(direct_payload["server"]["route"], "direct")

        self.assertEqual(_strip_time_relative(server_payload), _strip_time_relative(direct_payload))

    def test_direct_ignores_a_dead_server_url(self):
        """`--direct` must never construct a `ServerHub` at all -- not
        merely "prefer the hub". `KEEL_SERVER_URL` is set to a string
        `ServerHub.__init__` rejects outright (`ValueError`, no scheme/
        netloc) rather than a merely-unreachable address: `FallbackHub`'s
        own read fallback would silently mask a `--direct` that still
        built a `FallbackHub(ServerHub(...), github)` and just happened
        to have every read fall through to `github` anyway (verified:
        that weaker check -- a dead-but-well-formed URL -- still passes
        with the `--direct` early return in `build_hub` deleted, so it
        does not actually pin the property this test's name promises).
        A `ValueError` at construction time is not something a read
        fallback can hide: it fires before any FallbackHub reasoning
        applies, so `--direct` genuinely answering means `ServerHub` was
        never constructed at all.
        """
        self.seed_heartbeat("solo")
        dead_env = self.env(KEEL_SERVER_URL="not-a-url-at-all")
        result = self.run_cli("status", "--direct", "--json", env=dead_env)
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["server"], {"route": "direct"})
        self.assertEqual(payload["hosts"][0]["host"], "solo")

    def test_flags_override_env(self):
        """`--hub`/`--server`/`--token-file` win over their env vars."""
        wrong_env = self.env(
            FLEET_HUB_URL=str(self._root / "does-not-exist.git"),
            KEEL_SERVER_URL="http://127.0.0.1:1",
            KEEL_TOKEN_FILE=str(self._root / "no-such-token"),
        )
        result = self.run_cli(
            "status", "--direct", "--json",
            "--hub", self.state_url, "--code", self.code_url,
            env=wrong_env,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["server"], {"route": "direct"})

    def test_no_hub_url_is_a_clear_error(self):
        bare_env = self.hermetic_env()  # no FLEET_HUB_URL at all
        result = self.run_cli("status", "--direct", env=bare_env)
        self.assertEqual(result.returncode, 2)
        self.assertIn("FLEET_HUB_URL", result.stderr)

    def test_no_server_url_without_direct_is_a_clear_error(self):
        env = self.hermetic_env(FLEET_HUB_URL=self.state_url, FLEET_CODE_URL=self.code_url)
        result = self.run_cli("status", env=env)
        self.assertEqual(result.returncode, 2)
        self.assertIn("KEEL_SERVER_URL", result.stderr)


# ------------------------------------------------------------------------ #
# keel events
# ------------------------------------------------------------------------ #


class TestEvents(_KeelCliCase):
    def test_follow_receives_a_write(self):
        proc = self.popen_cli("events", "--follow", "--since", "0", "--timeout", "10", "--json")
        stderr_text = ""
        try:
            # Give the subprocess time to connect and open the SSE stream
            # before the write happens, so this genuinely exercises
            # "received AFTER connecting", not "replayed from `since`".
            time.sleep(0.6)
            self.events.append("test.followcheck", {"n": 42})
            line = _read_one_line(proc, timeout=10.0)
        finally:
            proc.terminate()
            try:
                _remaining_out, stderr_text = proc.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                _remaining_out, stderr_text = proc.communicate(timeout=5)

        self.assertIsNotNone(line, f"keel events --follow printed nothing; stderr={stderr_text}")
        row = json.loads(line)
        self.assertEqual(row["kind"], "test.followcheck")
        self.assertEqual(row["payload"], {"n": 42})
        self.assertEqual(row["seq"], 1)

    def test_non_follow_catches_up_and_exits(self):
        self.events.append("a", {"i": 1})
        self.events.append("b", {"i": 2})
        result = self.run_cli("events", "--since", "0", "--json", "--timeout", "1")
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
        self.assertEqual([(row["kind"], row["payload"]) for row in lines], [("a", {"i": 1}), ("b", {"i": 2})])

    def test_events_without_server_url_is_a_clear_error(self):
        env = self.hermetic_env(FLEET_HUB_URL=self.state_url, FLEET_CODE_URL=self.code_url)
        result = self.run_cli("events", env=env)
        self.assertEqual(result.returncode, 2)
        self.assertIn("KEEL_SERVER_URL", result.stderr)


# ------------------------------------------------------------------------ #
# keel desired show / set  (SPEC SS5.1's GET|PUT /v1/desired, from the
# operator's end of the wire; the route's own contract is pinned by
# tests/test_desired_route.py)
# ------------------------------------------------------------------------ #


class TestDesired(_KeelCliCase):
    """The CLI half of Stage 2 review finding F2.

    What each test would miss otherwise, checked by reverting one thing
    and re-running this class:
      * dropping `doc["generation"] = _next_generation(cur_payload)` from
        `server.py`'s `handle_desired_put` fails
        `test_set_via_the_server_bumps_the_generation_server_side` with
        `1 != 2` (the CLI's `mutate` edits the document it just read, so
        the PRE-IMAGE's generation rides along in the body and survives
        untouched) and `test_the_server_route_does_not_send_a_generation`
        with `4242 != 2`. The second is the one that pins WHERE the
        arithmetic happened: a client that computed it would also land
        the right number, so the only distinguishing evidence is a body
        generation the server had to overrule.
      * removing the `--direct` branch from `_build_desired_client` fails
        `test_set_direct_needs_no_server_at_all` (rc 1 with a `ValueError`
        traceback -- it runs with `KEEL_SERVER_URL` set to a string
        `ServerHub.__init__` rejects outright) and
        `test_set_direct_bumps_the_generation_client_side` on
        `'server' != 'direct'`.
    """

    def _desired(self) -> dict:
        """The landed document, read with a hub the CLI never touched."""
        return self.direct_hub.read("refs/fleet/desired")

    def test_show_on_an_absent_desired_says_so(self):
        result = self.run_cli("desired", "show", "--direct", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertIsNone(payload["sha"])
        self.assertIsNone(payload["desired"])

    def test_show_via_the_server_and_direct_agree(self):
        self.seed_desired({"m5": {"gates": 2, "agents": 1, "enabled": True}})
        via_server = json.loads(self.run_cli("desired", "show", "--json").stdout)
        via_direct = json.loads(self.run_cli("desired", "show", "--direct", "--json").stdout)
        self.assertEqual(via_server, via_direct)
        self.assertEqual(via_server["desired"]["generation"], 1)

    def test_set_creates_desired_when_it_does_not_exist_yet(self):
        result = self.run_cli("desired", "set", "--host", "m5", "--gates", "3", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["route"], "server")
        self.assertEqual(payload["desired"]["generation"], 1)
        self.assertEqual(self._desired()["hosts"]["m5"]["gates"], 3)

    def test_set_via_the_server_bumps_the_generation_server_side(self):
        self.seed_desired({"m5": {"gates": 1}})  # generation 1
        result = self.run_cli("desired", "set", "--host", "i7", "--gates", "2", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        landed = json.loads(result.stdout)["desired"]
        self.assertEqual(landed["generation"], 2)
        # Both hosts survive: `mutate` edits the document it just read.
        self.assertEqual(set(landed["hosts"]), {"m5", "i7"})
        self.assertEqual(self._desired(), landed)

    def test_the_server_route_does_not_send_a_generation(self):
        """A body generation the server did not compute must not survive.
        The CLI does not send one, so this drives the same route with a
        deliberately wrong one through `ServerHub.put_desired` and pins
        that the stored number came from the PRE-IMAGE."""
        self.seed_desired({"m5": {}})  # generation 1
        hub = ServerHub(self.server_url, token=self.token)
        sha, _doc = hub.read_desired()
        landed = hub.put_desired({"generation": 4242, "hosts": {"m5": {}}}, sha)
        self.assertEqual(landed["generation"], 2)
        self.assertEqual(self._desired()["generation"], 2)

    def test_set_direct_bumps_the_generation_client_side(self):
        self.seed_desired({"m5": {"gates": 1}})  # generation 1
        result = self.run_cli("desired", "set", "--host", "m5", "--gates", "5", "--direct", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["route"], "direct")
        self.assertEqual(payload["desired"]["generation"], 2)
        self.assertEqual(self._desired()["hosts"]["m5"]["gates"], 5)

    def test_set_direct_needs_no_server_at_all(self):
        """Same pin as `test_direct_ignores_a_dead_server_url`: a URL
        `ServerHub.__init__` REJECTS, not merely an unreachable one, so a
        `--direct` that still constructed a ServerHub would traceback."""
        env = self.env(KEEL_SERVER_URL="not-a-url-at-all::::")
        result = self.run_cli("desired", "set", "--host", "m5", "--gates", "1", "--direct", "--json", env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["route"], "direct")

    def test_disable_records_a_reason_and_enable_clears_it(self):
        self.run_cli("desired", "set", "--host", "m5", "--gates", "2")
        self.run_cli("desired", "set", "--host", "m5", "--disable", "--reason", "psu suspect")
        entry = self._desired()["hosts"]["m5"]
        self.assertIs(entry["enabled"], False)
        self.assertEqual(entry["reason"], "psu suspect")
        self.assertEqual(entry["gates"], 2, "disabling must not forget the targets")
        self.run_cli("desired", "set", "--host", "m5", "--enable")
        entry = self._desired()["hosts"]["m5"]
        self.assertIs(entry["enabled"], True)
        self.assertNotIn("reason", entry, "re-enabling clears the stale note (fleet up's behaviour)")

    def test_set_with_nothing_to_change_is_a_clear_error(self):
        result = self.run_cli("desired", "set", "--host", "m5")
        self.assertEqual(result.returncode, 2)
        self.assertIn("nothing to change", result.stderr)
        self.assertIsNone(self._desired())

    def test_set_without_a_server_or_direct_is_a_clear_error(self):
        env = self.env()
        env.pop("KEEL_SERVER_URL", None)
        result = self.run_cli("desired", "set", "--host", "m5", "--gates", "1", env=env)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--direct", result.stderr)
        self.assertIn("server-side", result.stderr)

    def test_set_never_re_issues_the_write_on_the_other_route(self):
        """With the server dead, a `desired set` over the server route
        FAILS rather than quietly writing to the hub: SPEC SS4.3 r2 for the
        ambiguous case, and for the reachable-server case the generation++
        would silently migrate back to this process. `--direct` is the
        deliberate way to choose the other semantics."""
        self.seed_desired({"m5": {"gates": 1}})
        self.server.stop()
        result = self.run_cli("desired", "set", "--host", "m5", "--gates", "9")
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("SS4.3 r2", result.stderr)
        self.assertEqual(
            self._desired()["hosts"]["m5"]["gates"], 1,
            "the write must not have landed by any route",
        )


# ------------------------------------------------------------------------ #
# keel server status / rehost
# ------------------------------------------------------------------------ #


class TestServer(_KeelCliCase):
    def test_status_reports_no_live_lease_initially(self):
        result = self.run_cli("server", "status", "--direct", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["route"], "direct")
        self.assertFalse(payload["lease_live"])
        self.assertIsNone(payload["lease"])

    def test_status_via_server_reports_health(self):
        result = self.run_cli("server", "status", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["route"], "server")
        self.assertIn("boot_id", payload["health"])
        self.assertIn("fallback", payload)
        self.assertEqual(payload["fallback"]["route"], "primary")

    def test_rehost_acquires_an_absent_lease_then_refuses(self):
        first = self.run_cli("server", "rehost", "--direct")
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertIn("acquired the server lease", first.stdout)

        # The lease is now live (fresh TTL) -- a second attempt refuses.
        second = self.run_cli("server", "rehost", "--direct")
        self.assertEqual(second.returncode, 3, second.stdout)
        self.assertIn("refusing", second.stderr)

        sha, payload = self.direct_hub.read_with_sha(SERVER_CLAIM_REF)
        self.assertIsNotNone(sha)
        self.assertFalse(is_expired(payload))

    def test_rehost_refuses_against_a_live_lease_from_someone_else(self):
        self.write_server_claim(expires_in_s=600, holder="other-host")
        result = self.run_cli("server", "rehost", "--direct")
        self.assertEqual(result.returncode, 3, result.stdout)
        self.assertIn("other-host", result.stderr)

    def test_the_lease_rehost_writes_parses_on_the_supported_python_floor(self):
        """ONE SPELLING for a lease deadline, across every writer.

        `cmd_server_rehost` wrote `started_at`/`expires_at` with
        `strftime('%Y-%m-%dT%H:%M:%SZ')` while `Claim._payload` and
        `election.ServerClaim` write `claim._iso` (a `+00:00` offset).
        Every reader parses with `datetime.fromisoformat`, which accepts a
        trailing `Z` only from Python 3.11 -- and docs/AGENT-SERVER-SPEC.md
        sets the floor at py >=3.10. MEASURED on /usr/bin/python3 (3.9.6,
        the same pre-3.11 semantics): `'2026-08-29T11:00:00Z'` raises
        `ValueError: Invalid isoformat string`; the `+00:00` form parses.

        `claim.is_expired` then fails OPEN on the value it cannot parse
        ("absence of a deadline is not itself a deadline"), so on a 3.10
        host an EXPIRED rehost-written lease read as LIVE for ever. That
        was cosmetic until Keel 3R-2 made this ref a scheduling input:
        `runner.AutonomyGate` watches it, and a lease stuck at LIVE means
        `autonomous_when_serverless` can never engage on the one host
        SPEC SS12 built it for.

        The instrument is a parser that rejects a trailing `Z` the way
        pre-3.11 `fromisoformat` does, applied to what the CLI actually
        wrote.
        """
        result = self.run_cli("server", "rehost", "--direct")
        self.assertEqual(result.returncode, 0, result.stderr)
        _sha, payload = self.direct_hub.read_with_sha(SERVER_CLAIM_REF)

        def parse_like_py310(raw: str):
            # `datetime.fromisoformat` before 3.11: no trailing `Z`.
            if raw.endswith("Z"):
                raise ValueError(f"Invalid isoformat string: {raw!r}")
            return datetime.fromisoformat(raw)

        for field in ("started_at", "expires_at"):
            with self.subTest(field):
                raw = payload[field]
                try:
                    parse_like_py310(raw)
                except ValueError as exc:
                    self.fail(
                        f"REHOST WROTE AN UNPARSEABLE DEADLINE: {field}={raw!r} "
                        f"cannot be read by `datetime.fromisoformat` on the "
                        f"py>=3.10 floor ({exc}); `claim.is_expired` then fails "
                        f"open and the lease reads LIVE for ever")
        # And byte-for-byte the shape every other writer of this ref uses.
        self.assertEqual(payload["expires_at"],
                         claim_iso(datetime.fromisoformat(payload["expires_at"])))

    def test_rehost_reacquires_an_expired_lease(self):
        self.write_server_claim(expires_in_s=-600, holder="long-gone-host")
        result = self.run_cli("server", "rehost", "--direct")
        self.assertEqual(result.returncode, 0, result.stderr)
        _sha, payload = self.direct_hub.read_with_sha(SERVER_CLAIM_REF)
        self.assertNotEqual(payload.get("holder_host"), "long-gone-host")
        self.assertFalse(is_expired(payload))


if __name__ == "__main__":
    unittest.main()
