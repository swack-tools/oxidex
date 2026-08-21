#!/usr/bin/env python3
"""Single source of truth for fleet-wide runtime defaults that must never
be hardcoded ad hoc across `tools/fleet` (R6, review of `staging/agent-
server` @ 99f06cb3).

Before this module existed, `doctor.py` and `ledger.py` each spelled
``"/tmp/oxidex-exiftool-cache"`` as their own ``os.environ.get(...)``
fallback, and `fleetd.py`/`gate.sh` split the identical literal into a
basename variable plus an ``f"/tmp/{basename}"``/``"/tmp/$basename"``
reconstruction -- a "two-piece assembly" that is byte-identical at
runtime to spelling the path out, but was written specifically to dodge
`tests/test_no_hardcoded_hosts.py`'s substring fence (see that file's own
docstring, and its own admission in `fleetd.py`'s
`_exiftool_cache_dir()`/`_default_exiftool_cache_dir()` docstring: "Built
from two pieces ... so this line is not itself a hardcoded-host match").
Both shapes are the same bug: the default lived in more than one place,
so changing it required finding every copy by hand, and a copy that
merely *looks* different from the others (the two-piece one) is exactly
as easy to miss as a byte-for-byte duplicate.

This module (Python) and `units/fleet-env.sh` (shell, sourced rather than
imported) are the ONLY two places `DEFAULT_EXIFTOOL_CACHE_DIR`'s value is
allowed to be written out. Every other file imports one or sources the
other instead of re-deriving it -- enforced by
`tests/test_no_hardcoded_hosts.py`, which now also forbids the two-piece
idiom, not just the single contiguous literal.

Standard library only; no side effects at import time.
"""

from __future__ import annotations

import os
from pathlib import Path

# The one Python-side place this literal is allowed to be written out.
# Mirrored, byte-for-byte, in units/fleet-env.sh's own default -- a test
# in test_no_hardcoded_hosts.py pins the two against each other so they
# cannot silently drift apart.
DEFAULT_EXIFTOOL_CACHE_DIR = "/tmp/oxidex-exiftool-cache"


def exiftool_cache_dir() -> Path:
    """`EXIFTOOL_CACHE_DIR` from the environment, or the fleet default.

    Every consumer of the pinned-oracle cache directory should call this
    (or read `DEFAULT_EXIFTOOL_CACHE_DIR` directly, for the rarer case of
    needing the default even when the environment overrides it) instead
    of re-deriving the fallback itself.
    """
    return Path(os.environ.get("EXIFTOOL_CACHE_DIR", DEFAULT_EXIFTOOL_CACHE_DIR))


# ---------------------------------------------------------------------
# R2 (review finding): the PAT file at this default path is otherwise
# inert unless FLEET_GIT_TOKEN_FILE is also exported -- every hand-run
# step (install_secrets.sh, seed_desired.py, `fleet up`, `fleet status
# --why`, a hand-started fleetd) that forgets to export it silently loses
# the credential. `tools/fleet/rollout/install_secrets.sh` already
# defaults its own `--token-file` to this exact path; this constant lets
# `doctor.py`'s health check (and, per R2, eventually
# `fleetlib.credential_env` itself -- out of scope for this change) agree
# with that default instead of re-spelling it.
# ---------------------------------------------------------------------


def default_git_token_file() -> Path:
    """``~/.keel/secrets/git-token`` -- the exact path
    `install_secrets.sh` defaults ``--token-file`` to and every
    `units/*` template sets ``FLEET_GIT_TOKEN_FILE`` to.

    ``$HOME`` overrides for hermetic tests (same convention `doctor.py`'s
    `check_disk()` already uses), falling back to `Path.home()` only when
    unset.
    """
    home = Path(os.environ.get("HOME", str(Path.home())))
    return home / ".keel" / "secrets" / "git-token"
