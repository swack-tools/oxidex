"""ONE hermetic process environment for every fleet fixture.

WHY THIS FILE EXISTS. Gate `keel1` on the i7 (Linux) ran the fleet suite
red on `staging/agent-server@90cb01e4` while the identical suite was
642/642 green on the m5 (macOS). None of the fourteen failures was a
fleet bug. Every one was the *invoking environment* reaching into a
fixture that believed itself hermetic:

  1. `gate.sh` exports `FLEET_HUB_URL` (and `units/fleet-env.sh`, which
     it sources, exports `EXIFTOOL_CACHE_DIR` and
     `FLEET_VERDICT_STORE_FAILED_SUFFIX`) and then runs this suite as a
     child. Every fixture that built a subprocess env as
     `{**os.environ, ...}` -- or ran an entry point in-process, where
     `train.main`, `cli.main`, `agentworker.main`, `fleetd.main` and
     `install_secrets.sh` all default their arguments from
     `os.environ` -- inherited the gate's real hub. `install_secrets.sh`
     saw `FLEET_HUB_URL=/home/allen/git/afx-local.git` and refused it as
     "not https", so the usage-error test never saw the usage error;
     `test_verdict_marker_seam`'s `bash -c` inherited the real suffix, so
     its "unset variable" control could not unset it.
  2. Fixtures redirect `HOME` into the tempdir (correctly -- never the
     developer's `~/.gitconfig`, `~/.keel/secrets`, `~/.fleetd`) but did
     not pin a git identity for the code under test. The train's own
     `git merge`/`git commit` in its scratch clone then fell back to
     git's ident auto-detection, which SUCCEEDS on a macOS host whose
     hostname carries a domain (`allen@Allens-Air.lan`) and is FATAL on a
     Linux host whose does not (`allen@server.(none)`: "unable to
     auto-detect email address"). `train.merge_members` reported the
     failed merge as `merge-conflict`, and ten train tests ejected their
     one branch with a reason that pointed everyone at the wrong repo.

Both are the same defect: the hermetic boundary was drawn around the
filesystem (tempdir, redirected HOME) and not around the environment.
This module draws the second line, once, so that no fixture has to get
it right on its own again.

WHAT IT DOES.
  * `scrub_env()` -- a COPY of the environment with every fleet-shaped
    variable removed (`FLEET_*`, `KEEL_*`, `FLEETD_*`, the pinned-oracle
    variables gate.sh exports, every ssh/askpass override git honours)
    and a fixed git identity installed. The handful of knobs that
    configure the TEST RUN itself (`FLEET_TESTS_HERMETIC`, the live
    opt-ins) are kept; see `KEEP`.
  * `HermeticEnvMixin` -- applies the same scrub to `os.environ` for the
    duration of each test (for in-process entry points), restores the
    whole environment afterwards, and offers `self.hermetic_env(...)`
    for every subprocess the fixture spawns.

Both routes share `scrub_env`, so a variable added to the scrub list is
scrubbed for in-process calls and for subprocesses alike.

`tests/test_env_hermetic.py` is the fence: it runs the modules that went
red under a deliberately poisoned environment and requires green.
"""

from __future__ import annotations

import os
import unittest
from typing import Dict, Iterable, Mapping, Optional

# Variables the fleet's ENTRY POINTS default their configuration from
# (argparse defaults, `os.environ.get(...)` in train/cli/fleetd/
# agentworker/doctor/seed_desired, `${FLEET_HUB_URL:-}` in the shell
# scripts) -- anything with these prefixes describes a REAL deployment,
# never a fixture.
SCRUB_PREFIXES = ("FLEET_", "KEEL_", "FLEETD_")

# Exact names: what `gate.sh` / `units/fleet-env.sh` export around the
# suite, plus every environment variable through which git would pick up
# an ambient transport or credential. `GIT_DIR`/`GIT_WORK_TREE`/
# `GIT_INDEX_FILE` are in here because a leaked one silently redirects
# EVERY git command a fixture runs at some other repository.
SCRUB_EXACT = frozenset({
    "EXIFTOOL_CACHE_DIR", "EXIFTOOL", "OXIDEX", "TAGMATRIX_WORK",
    "GIT_SSH_COMMAND", "GIT_SSH", "GIT_ASKPASS", "SSH_ASKPASS",
    "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE",
    "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM",
})

# Knobs that configure THIS TEST RUN rather than a deployment. They
# survive the scrub so that `FLEET_TESTS_HERMETIC=1` keeps gating the
# network-touching tests and the live opt-ins keep opting in.
KEEP_PREFIXES = ("FLEET_TESTS_", "FLEET_LIVE_")
KEEP_EXACT = frozenset({"FLEET_SEAMS_SLOW", "FLEET_TEST_HUB_URL", "FLEET_TEST_HUB"})

# The identity every fixture's own git calls already used; pinned here for
# the CODE UNDER TEST as well, whose merges and commits otherwise depend
# on whether the host's hostname happens to carry a domain.
GIT_IDENTITY: Dict[str, str] = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
}


def is_scrubbed(name: str) -> bool:
    """True if `name` is removed by `scrub_env` (before `extra` is applied)."""
    if name in KEEP_EXACT or name.startswith(KEEP_PREFIXES):
        return False
    return name in SCRUB_EXACT or name.startswith(SCRUB_PREFIXES)


def scrubbed_keys(env: Optional[Mapping[str, str]] = None) -> list:
    """The names `scrub_env` would drop from `env` (default `os.environ`),
    sorted -- what a fixture can print when it wants to say WHICH leak it
    is refusing."""
    src = os.environ if env is None else env
    return sorted(k for k in src if is_scrubbed(k))


def scrub_env(base: Optional[Mapping[str, str]] = None, **extra: str) -> Dict[str, str]:
    """A new environment dict: `base` (default `os.environ`) minus every
    fleet-shaped variable, plus `GIT_IDENTITY`, plus `extra`.

    `extra` wins over everything, so a fixture that deliberately sets
    `FLEET_HOST` or `FLEET_AGENT_CLI_OVERRIDE` for the process it spawns
    keeps it: the scrub removes what the INVOKER leaked, never what the
    FIXTURE chose. Values in `extra` are coerced to `str` (paths,
    numbers) because `subprocess` rejects anything else.
    """
    src = os.environ if base is None else base
    out = {k: v for k, v in src.items() if not is_scrubbed(k)}
    out.update(GIT_IDENTITY)
    out.update({k: str(v) for k, v in extra.items()})
    return out


def apply_to_os_environ() -> "callable":
    """Scrub `os.environ` IN PLACE and return a zero-argument restorer that
    puts the whole environment back exactly as it was.

    For fixtures that cannot inherit `HermeticEnvMixin` (module-level
    setup, a `setUpClass`). Test classes should use the mixin instead.
    """
    saved = dict(os.environ)
    for name in scrubbed_keys():
        os.environ.pop(name, None)
    os.environ.update(GIT_IDENTITY)

    def restore() -> None:
        os.environ.clear()
        os.environ.update(saved)

    return restore


class HermeticEnvMixin:
    """Scrubs `os.environ` for the duration of every test and restores it
    afterwards; offers `hermetic_env()` for spawned processes.

    Put it FIRST in the bases and call `super().setUp()` FIRST in your own
    `setUp`, so that HOME redirection and the rest of the fixture happen
    on top of the scrubbed environment rather than underneath it:

        class _FixtureCase(HermeticEnvMixin, unittest.TestCase):
            def setUp(self):
                super().setUp()
                os.environ["HOME"] = ...

    Restoration is an `addCleanup`, which unittest runs AFTER `tearDown`:
    a fixture's own tearDown that puts individual keys back still works
    and is then superseded by the full snapshot. A class that defines
    `setUpClass` must call `super().setUpClass()` first for the same
    reason. `tests/test_env_hermetic.py` enforces both rules over every
    `test_*.py` in this directory by reading their ASTs.
    """

    @classmethod
    def setUpClass(cls) -> None:  # noqa: N802 -- unittest's spelling
        """Class-level fixtures (`setUpClass` hubs, ledgers, gate harnesses)
        run before any `setUp`, so the scrub is applied here too and undone
        by an `addClassCleanup` after the last test of the class."""
        cls.addClassCleanup(apply_to_os_environ())
        super().setUpClass()

    def setUp(self) -> None:  # noqa: N802 -- unittest's spelling
        self.addCleanup(apply_to_os_environ())
        super().setUp()

    def hermetic_env(self, **extra: str) -> Dict[str, str]:
        """`scrub_env(**extra)` -- the env for any subprocess this test
        spawns. Taken from the CURRENT `os.environ`, so the fixture's own
        HOME redirection and `FLEET_*` choices made after `setUp` are in
        it; only an invoker's leak is not."""
        return scrub_env(**extra)

    def assertEnvHermetic(self, env: Optional[Mapping[str, str]] = None,  # noqa: N802
                          allow: Iterable[str] = ()) -> None:
        """Fail if `env` (default `os.environ`) still carries a scrubbed
        name other than those in `allow` -- the fixture's own deliberate
        settings."""
        allowed = set(allow)
        leaked = [k for k in scrubbed_keys(env) if k not in allowed]
        if leaked:
            raise AssertionError(
                f"environment is not hermetic; leaked from the invoker: {leaked}")


class HermeticCase(HermeticEnvMixin, unittest.TestCase):
    """`unittest.TestCase` with the hermetic environment already applied."""
