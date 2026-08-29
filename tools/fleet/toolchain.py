#!/usr/bin/env python3
"""THE toolchain resolver: one definition of *which* `rustc` this fleet
measures and of *how* its output becomes a `rustc_id` / `platform_id`.

WHY THIS FILE EXISTS -- the Keel Stage 1 LIVE incident, 2026-08-27/28.

On the i7 (`server`), fleetd's host claim recorded

    platform_id b2bdf493bcf6d1dc55181a0efc2774b6d9cf7bf5c8dcc5bed592c994b2ad38c4

while the gate that *the same fleetd had just spawned* wrote its verdict,
the same minute on the same host, under

    platform_id b6613b194bef01c71d7e040e4a15ba8999e2b8263b61c09e156819ccb213485a

`platform_id` is one third of the verdict cache key
(`verdict.verdict_ref`), so `verdict.lookup` under fleetd's key returned
`None` for a tree whose PASS was sitting on the state repo under the
gate's key. `classify_branch` therefore never returned AWAITING_TRAIN,
and a host with `gates >= 1` re-gated the identical merge tree forever --
~21 minutes a pass -- while a correct, published PASS went unread.

THE ROOT CAUSE WAS NOT WHICH COMPILER. Both sides resolved the same
rustc 1.97.1 out of `~/.cargo/bin` (`claim._rustc_vv` already prepended
it). The two ids differed by ONE TRAILING NEWLINE:

  * `gate.sh` captured the text with `RUSTC_VV=$(rustc -vV)`. Command
    substitution strips trailing newlines, and `printf '%s'` adds none
    back, so the gate hashed the output WITHOUT its final `\\n`.
  * `claim.compute_platform_id` hashed `subprocess`'s `stdout` verbatim,
    WITH the final `\\n`.

Measured on the real i7, in fleetd's own environment:

    sha256(vv)              = b2bdf493...   <- what fleetd stored
    sha256(vv.rstrip("\\n"))  = b6613b19...   <- what the gate stored

Three implementations existed, and no two of them agreed on both fields:

               platform_id   rustc_id
    gate.sh    b6613b19      b5d14336
    claim.py   b2bdf493      12562484
    verdict.py b2bdf493      b5d14336

`doctor.CANONICAL_TOOLCHAIN_ID` is `b5d14336...` -- i.e. `gate.sh`'s
formula is the one the fleet's own pinned constant was derived under, so
that is the formula this module canonicalizes, and adopting it leaves
every verdict already published on the state repo readable. Nothing has
to be re-gated.

WHAT IS SHARED, AND HOW.

  * THE FORMULA lives here and ONLY here. `claim.compute_platform_id`,
    `claim.compute_rustc_id` and `verdict.compute_ids` are now thin
    delegations; `gate.sh` no longer computes a digest at all -- it
    sources `units/fleet-toolchain.sh`, whose `fleet_toolchain_ids`
    shells out to this module's `ids` subcommand. There is exactly one
    implementation, so the scheduler and the gate it spawns cannot
    disagree by construction.
  * THE PATH PREFIX (`$HOME/.cargo/bin:$HOME/.local/bin`) is spelled
    twice on purpose -- here in Python and in `units/fleet-toolchain.sh`
    in shell -- for the same reason `config.DEFAULT_EXIFTOOL_CACHE_DIR`
    and `units/fleet-env.sh` mirror one literal: a shell script must be
    able to build its own PATH without first succeeding at a Python
    subprocess, or a failed subprocess would silently drop `~/.cargo/bin`
    and build the whole gate with the wrong compiler.
    `tests/test_toolchain_seam.py` pins the two spellings against each
    other, and -- more importantly -- drives the REAL `gate.sh` helper
    end to end and compares its output to `compute_ids` here.

Standard library only. No side effects at import time. Nothing in here
talks to a hub.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import Optional, Tuple

# ---------------------------------------------------------------------
# Which rustc
# ---------------------------------------------------------------------

# Prepended to PATH, in this order, before resolving `rustc`. A login
# shell on the Macs finds a Homebrew rustc off `/opt/homebrew/bin` ahead
# of `~/.cargo/bin` (see `doctor.gate_path_env`'s comment), and a systemd
# --user service on the i7 inherits a PATH with no `~/.cargo/bin` in it
# at all, so "whatever PATH we happened to be started with" is not an
# answer -- it is how the same host ends up with two toolchains.
#
# Mirrored, segment for segment, by `units/fleet-toolchain.sh`'s
# `FLEET_TOOLCHAIN_PATH_PREFIX` default; pinned against it by
# `tests/test_toolchain_seam.py`.
TOOLCHAIN_PATH_PREFIX_REL: Tuple[str, ...] = (".cargo/bin", ".local/bin")

# Escape hatch for a host whose rustc is somewhere else entirely. Read
# per call, never cached, so a test can set it.
RUSTC_ENV = "FLEET_RUSTC"


def _home(env: Optional[dict] = None) -> str:
    src = os.environ if env is None else env
    return src.get("HOME") or os.path.expanduser("~")


def toolchain_path_prefix(env: Optional[dict] = None) -> str:
    """`$HOME/.cargo/bin:$HOME/.local/bin` -- the ONE Python spelling."""
    home = _home(env)
    return os.pathsep.join(str(Path(home) / rel) for rel in TOOLCHAIN_PATH_PREFIX_REL)


def toolchain_env(env: Optional[dict] = None) -> dict:
    """A copy of `env` (default `os.environ`) with the fleet toolchain
    prefix in front of PATH. This is the environment every fleet
    component must resolve `rustc`/`cargo` under."""
    src = dict(os.environ if env is None else env)
    prefix = toolchain_path_prefix(src)
    src["PATH"] = prefix + os.pathsep + src.get("PATH", "")
    return src


def resolve_rustc(env: Optional[dict] = None) -> Optional[str]:
    """Absolute path of the `rustc` this fleet measures, or None.

    `FLEET_RUSTC` wins outright; otherwise the first `rustc` on
    `toolchain_env()`'s PATH.
    """
    src = os.environ if env is None else env
    override = (src.get(RUSTC_ENV) or "").strip()
    if override:
        return override
    resolved = toolchain_env(src)
    return shutil.which("rustc", path=resolved["PATH"])


# ---------------------------------------------------------------------
# The text, and the normalization
# ---------------------------------------------------------------------


def normalize_vv(text: str) -> str:
    """`rustc -vV` output as `gate.sh` sees it.

    `RUSTC_VV=$(rustc -vV)` -- command substitution strips EVERY trailing
    newline. This one line is the whole 2026-08-27 incident: hashing the
    un-normalized text yields a different `platform_id` for the identical
    compiler, and therefore a verdict-cache key nothing else on the host
    ever writes.
    """
    return text.rstrip("\n")


def rustc_vv(env: Optional[dict] = None, timeout: int = 10) -> str:
    """Normalized `rustc -vV` text, or `""` if rustc cannot be run.

    Empty rather than an exception, deliberately: callers hash whatever
    they get, so a host with no compiler still has a stable (if useless)
    identity instead of a crash inside a reconcile loop. `gate.sh`'s
    `rustc -vV 2>/dev/null` degrades the same way.
    """
    src = os.environ if env is None else env
    rustc = resolve_rustc(src)
    if not rustc:
        return ""
    try:
        result = subprocess.run(  # nosec B603 -- list argv, no shell
            [rustc, "-vV"], capture_output=True, timeout=timeout,
            env=toolchain_env(src),
        )
    except (OSError, subprocess.TimeoutExpired):
        return ""
    if result.returncode != 0:
        return ""
    return normalize_vv(result.stdout.decode("utf-8", "replace"))


# ---------------------------------------------------------------------
# The ids
# ---------------------------------------------------------------------


def compute_ids(rustc_vv_text: str) -> Tuple[str, str]:
    """`(rustc_id, platform_id)` from `rustc -vV` text, however that text
    was obtained (already normalized, or straight off a pipe).

    Both halves reproduce `gate.sh` exactly:

        PLATFORM_ID=$(printf '%s' "$RUSTC_VV" | sha256)
        RUSTC_ID=$(printf '%s\\n' "$RUSTC_VV" | grep -v '^host:' | sha256)

    so `platform_id` is the digest of the normalized text and `rustc_id`
    is the digest of that text with every `^host:` line dropped and a
    newline re-appended to each surviving line -- `grep`'s output shape,
    not `"\\n".join(...)`, which differs from it whenever the last line
    would otherwise lose its terminator.

    `split("\\n")` and not `splitlines()`: `printf '%s\\n' ""` emits one
    empty line, which `grep -v '^host:'` keeps, so an EMPTY toolchain
    hashes `"\\n"` and not `""`. `splitlines()` would return `[]` there
    and silently pick the other answer.
    """
    text = normalize_vv(rustc_vv_text)
    platform_id = hashlib.sha256(text.encode("utf-8")).hexdigest()
    kept = [line for line in text.split("\n") if not line.startswith("host:")]
    stripped = "".join(line + "\n" for line in kept)
    rustc_id = hashlib.sha256(stripped.encode("utf-8")).hexdigest()
    return rustc_id, platform_id


def compute_platform_id(rustc_vv_text: Optional[str] = None,
                        env: Optional[dict] = None) -> str:
    """sha256(`rustc -vV`), host line INCLUDED. "Is this verdict
    transferable to that host?" -- part of the verdict cache key."""
    text = rustc_vv(env) if rustc_vv_text is None else rustc_vv_text
    return compute_ids(text)[1]


def compute_rustc_id(rustc_vv_text: Optional[str] = None,
                     env: Optional[dict] = None) -> str:
    """sha256(`rustc -vV`) with the `host:` line stripped. "Is this host
    on the canonical compiler?" -- matched against
    `doctor.CANONICAL_TOOLCHAIN_ID`."""
    text = rustc_vv(env) if rustc_vv_text is None else rustc_vv_text
    return compute_ids(text)[0]


def probe(env: Optional[dict] = None) -> dict:
    """Everything an instrument header (or a mismatch report) needs, from
    ONE `rustc -vV` -- so the path, the release line and the two digests
    cannot describe different runs of the compiler."""
    src = os.environ if env is None else env
    rustc = resolve_rustc(src)
    text = rustc_vv(src)
    rustc_id, platform_id = compute_ids(text)
    release = next((l for l in text.split("\n") if l.startswith("release:")), "")
    return {
        "rustc_path": rustc or "",
        "path_prefix": toolchain_path_prefix(src),
        "release": release,
        "rustc_id": rustc_id,
        "platform_id": platform_id,
    }


# ---------------------------------------------------------------------
# CLI -- what `units/fleet-toolchain.sh` shells out to
# ---------------------------------------------------------------------

# The KEY=value lines `--format sh` emits, in order. Kept as a tuple so
# the shell helper can be strict about what it accepts (it parses with a
# `case`, never `eval`) and so a test can assert the contract without
# re-listing it.
SH_KEYS = ("FLEET_TOOLCHAIN_PATH_PREFIX", "RUSTC_PATH", "RUSTC_ID", "PLATFORM_ID")


def _cli_ids(args: argparse.Namespace) -> int:
    info = probe()
    if args.format == "json":
        print(json.dumps(info, sort_keys=True))
        return 0
    values = {
        "FLEET_TOOLCHAIN_PATH_PREFIX": info["path_prefix"],
        "RUSTC_PATH": info["rustc_path"],
        "RUSTC_ID": info["rustc_id"],
        "PLATFORM_ID": info["platform_id"],
    }
    for key in SH_KEYS:
        print(f"{key}={values[key]}")
    return 0


def main(argv: Optional[list] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    ids_p = sub.add_parser("ids", help="this host's toolchain identity")
    ids_p.add_argument("--format", choices=("sh", "json"), default="sh")
    ids_p.set_defaults(func=_cli_ids)
    ns = parser.parse_args(argv)
    return ns.func(ns)


if __name__ == "__main__":
    raise SystemExit(main())
