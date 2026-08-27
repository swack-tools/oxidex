"""Shared `FLEET_TEST_HUB=bare|server` switch (PLAN Stage 2 task 7).

WHAT THIS IS FOR. Every fixture in this directory that needs a coordination
hub already builds its own throwaway `git init --bare` state repo (and
sometimes a second one for `code_url`) and hands it to `fleetlib.Hub`
directly. That is still true after this module exists -- `make_hub` never
creates a repo itself. What it decides is HOW a test talks to the repo it
was already given:

  * `FLEET_TEST_HUB=bare` (the default, and today's only behaviour):
    `make_hub` returns a plain `fleetlib.Hub(hub_path, workdir=..., code_url=...)`
    -- byte-identical to the `Hub(...)` call it replaces.
  * `FLEET_TEST_HUB=server`: a real `keel-server` (`keel/server.py`'s
    `KeelHTTPServer`, not a stub) is started on `127.0.0.1:0`, backed by a
    `CachedHub` (`keel/cachedhub.py`) fronting the SAME bare repo through
    `keel/hubstore.py`'s adapter (`build_store`) -- the real stack `keel
    status`/a runner would talk to, not a store double. `make_hub` returns
    `FallbackHub(ServerHub(<fixture's own url>), Hub(hub_path, ...))`
    (SPEC SS4.2/SS4.3, `keel/fallbackhub.py`, `keel/serverhub.py`): the exact
    shape production code consumes, so every test that builds its hub
    through this switch exercises the wire -- serialization, status-code
    mapping, r1 (fresh claims), r2 (no write-retry after an ambiguous
    failure) -- not only `test_serverhub.py`'s own dedicated cases.

USAGE, from any fixture's `setUp` (mechanical, one line):

    self.hub = _fixtures.make_hub(self, self.hub_path, workdir=self.workdir)

`case` is the `unittest.TestCase` instance; `make_hub` registers the
fixture server's teardown via `case.addCleanup`, so no fixture has to add
its own cleanup line, and cleanup runs (best-effort, see `_ServerHubFixture
.close`) even when the fixture's own `tearDown` already deleted the
tempdir the bare repo lived under.

`FLEET_TEST_HUB` is read once, at import time, exactly like
`test_fleetlib.py` reads `FLEET_TEST_HUB_URL` once at its own import time --
a fixture that wants to force one mode regardless of the environment (a
test that only makes sense against a bare repo, for example) passes
`mode=` explicitly. `FLEET_TEST_HUB` must survive `tests/_env.py`'s
hermetic scrub -- it configures the TEST RUN, not a deployment -- and is
listed in `_env.KEEP_EXACT` beside `FLEET_TEST_HUB_URL` for exactly that
reason.

WHAT SERVER MODE DOES NOT DO. It does not make the fixture's bare repo(s)
for it, does not seed any refs, and does not touch `code_push_url`/
`tip_push_url` (those stay the GitHub half's, per `FallbackHub`, unchanged
from today). A fixture whose test methods write to the state repo through
some OTHER route than the returned hub -- a raw `git push` from a helper
thread, a second `fleetlib.Hub` instance built by the test itself -- will
only see that write once the fixture server's periodic sweep catches up
(`_SWEEP_INTERVAL_S`, short but not zero); that gap is a real property of
the production server too, not an artifact of this fixture.
"""

from __future__ import annotations

import hashlib
import os
import shutil
import sys
import tempfile
import uuid
from pathlib import Path
from typing import Optional, Union

_FLEET_DIR = Path(__file__).resolve().parents[1]  # tools/fleet
_KEEL_DIR = _FLEET_DIR / "keel"
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

from fleetlib import Hub  # noqa: E402
import server as keel_server  # noqa: E402  -- keel/server.py; top-level like test_serverhub.py imports it
from keel.fallbackhub import FallbackHub  # noqa: E402
from keel.hubstore import build_store  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402

__all__ = ["MODE", "MODES", "make_hub"]

MODES = ("bare", "server")

# Read once, like `test_fleetlib.LIVE_HUB_URL`. `.strip().lower()` so
# `FLEET_TEST_HUB=Server` (a shell typo, a CI YAML quirk) still matches
# rather than silently falling through to "bare".
MODE = (os.environ.get("FLEET_TEST_HUB", "bare").strip().lower() or "bare")

# The production sweep is 30s (SPEC SS3.2); tests run in seconds, and a
# fixture whose test writes through some route OTHER than the hub this
# module returns (a raw `git push`, a second `Hub`) wants that write
# visible to the server well inside whatever poll loop the test itself
# uses (most of this suite's own polling loops use a 0.1-0.5s interval).
_SWEEP_INTERVAL_S = 0.5

# One fixture-only role token, minted fresh per process. Never logged
# (`ServerHub.__repr__` and `TokenStore` both refuse to); "runner" satisfies
# every route this suite exercises (`GET` routes accept any authenticated
# role, `POST/PUT/DELETE /v1/refs/*` require runner-or-operator).
_FIXTURE_ROLE = "runner"
_FIXTURE_TOKEN = "fixture-" + uuid.uuid4().hex


def _sha256_hex(raw: str) -> str:
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


class _ServerHubFixture:
    """Owns one fixture `keel-server` (a real `KeelHTTPServer`, bound to
    `127.0.0.1:0`) plus the `CachedHub` that fronts `hub_path`/`code_url`
    for it, for the duration of one test. `.hub` is the `FallbackHub` a
    test should use in its place; `.close()` tears both down and is
    best-effort by construction -- mirrors `test_fleetlib._sweep`'s own
    reasoning: a fixture's `tearDown` may already have deleted the tempdir
    the bare repo lived under by the time this runs, and a cleanup that
    raises would mask the test's own result.
    """

    def __init__(self, hub_path: str, workdir: Path, code_url: Optional[str]):
        # The server's OWN index cache lives entirely OUTSIDE `workdir`:
        # `workdir` is what the caller's test methods know as "the local
        # git cache" (several reach in directly, e.g. `subprocess.run(["git",
        # "--git-dir", self.workdir, ...])`), and in `bare` mode that is
        # exactly what `fleetlib.Hub.__init__` `git init --bare`s it into.
        # Nesting the server's cache under it (an earlier version of this
        # fixture did) leaves `workdir` itself un-initialised in `server`
        # mode -- "not a git repository" the moment such a test runs -- for
        # no benefit, since nothing needs the two caches to be related.
        self._server_tmp = tempfile.mkdtemp(prefix="keel-fixture-server-")
        self._store = build_store(
            hub_path,
            Path(self._server_tmp) / "index-cache",
            code_url=code_url,
            sweep_interval=_SWEEP_INTERVAL_S,
        )
        tokens = keel_server.TokenStore(
            [{"id": "fixture", "role": _FIXTURE_ROLE, "sha256": _sha256_hex(_FIXTURE_TOKEN)}]
        )
        self._events = keel_server.EventLog(":memory:")
        config = keel_server.ServerConfig(
            bind_host="127.0.0.1",
            port=0,
            # Fixture servers live seconds; the watchdog restarting a
            # perfectly healthy accept loop mid-test would only add noise.
            watchdog_timeout=100.0,
            watchdog_check_interval=100.0,
        )
        self._server = keel_server.build_server(config, store=self._store, tokens=tokens, events=self._events)
        self._server.start()
        port = self._server.server_address[1]
        primary = ServerHub(f"http://127.0.0.1:{port}", token=_FIXTURE_TOKEN)
        # The fallback half sits at `workdir` itself -- byte-identical to
        # the plain `Hub(hub_path, workdir=workdir, ...)` the `bare` branch
        # returns -- so a test that treats `workdir` as a raw local git
        # cache sees the same thing in both modes.
        fallback = Hub(url=hub_path, workdir=str(workdir), code_url=code_url)
        self.hub = FallbackHub(primary, fallback)

    def close(self) -> None:
        for step in (self._server.stop, self._store.close, self._events.close):
            try:
                step()
            except Exception:
                pass
        shutil.rmtree(self._server_tmp, ignore_errors=True)


def make_hub(
    case,
    hub_path: Union[str, "os.PathLike[str]"],
    *,
    workdir: Union[str, "os.PathLike[str]"],
    code_url: Optional[str] = None,
    mode: Optional[str] = None,
):
    """The `Hub`-shaped object `case` should use in place of a direct
    `fleetlib.Hub(...)` construction, per `FLEET_TEST_HUB` (or `mode`, to
    force one regardless of the environment).

    `case` is the `unittest.TestCase` instance whose `addCleanup` will stop
    the fixture server, when one is started; `hub_path`/`code_url` are
    whatever bare repo(s) the caller already built with `git init --bare`.
    """
    chosen = (mode or MODE)
    if chosen not in MODES:
        raise ValueError(f"FLEET_TEST_HUB={chosen!r} must be one of {MODES}")
    workdir = Path(workdir)
    if chosen == "bare":
        return Hub(url=str(hub_path), workdir=str(workdir), code_url=code_url)
    fixture = _ServerHubFixture(str(hub_path), workdir, code_url)
    case.addCleanup(fixture.close)
    return fixture.hub
