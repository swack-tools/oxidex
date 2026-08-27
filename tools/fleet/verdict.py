#!/usr/bin/env python3
"""Verdict cache (FLEET_SPEC.md M6, T1.2).

This is the phase that closes the one *correctness* hole in the fleet plan
(FLEET_SPEC.md §5: "P2 ... is what stops a wrong-but-green merge reaching
the tip"): a content-addressed cache for gate verdicts, keyed by
`(merge-result tree sha, gate_version, platform_id)`. Two hosts that happen
to compute the same merge derive the same key, so the second one reuses the
first's verdict instead of re-running a 20-45 minute gate.

NOTE (ARCH-FIX R9): this module used to also carry `is_admissible()` --
whether a *cached* verdict still speaks for the tree that would exist if its
branch were merged to a moved tip. It was deleted 2026-08-15: zero
production callers ever invoked it (`gate.sh` only ever calls `lookup`/
`store`), so the protection it described never actually ran. See
`docs/FLEET.md`'s M3 note and the deletion commit for the superseding
design (tree-keyed verdict caching + LLM convergence agents).

Two identity fields travel with every verdict, per the T0.1/T0.3 toolchain
skew this plan resolves (FLEET_SPEC.md §7 addenda, "Two tasks computed
`toolchain_id` differently, and both were right"):

  * `rustc_id`    = sha256(`rustc -vV`) with the `host:` line stripped.
                    "Is this host on the canonical compiler?"
  * `platform_id` = sha256(`rustc -vV`) unstripped. "Is this verdict
                    transferable to that host?" Part of the cache key --
                    collapsing the two would let a Linux PASS on
                    `ffi_c_integration` silently satisfy a macOS gate slot,
                    which is the exact cross-platform skew that cost a day.

Standard library only. `Hub` (imported from `fleetlib`, T0.2's contract --
not redefined here) is the only thing that ever talks to a remote.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path
from typing import List, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402

# Qualified `keel.<name>` imports, not bare ones -- keel/cli.py's own
# comment explains why: a bare `import fallbackhub`/`import serverhub`
# here would risk a second, distinct module object (and therefore a
# second, distinct exception class) alongside whatever this same process
# imported qualified elsewhere (e.g. a runner that also calls into
# verdict.py's functions in-process rather than as a subprocess).
from keel.fallbackhub import FallbackHub  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402

SCHEMA_VERSION = 1

# result: PASS | FAIL | ABORT (FLEET_SPEC.md §7 addenda).
#
# ABORT covers infrastructure failure that says nothing about the branch's
# correctness -- OOM, low disk, a lost/degraded oracle, a killed process --
# and is deliberately kept out of both `PASS` and `FAIL`'s consequences: the
# cache never *serves* one as a settled answer (unlike a real FAIL), so it
# schedules a retry instead of condemning the branch. See `lookup()` and
# `store()` below.
RESULT_PASS = "PASS"
RESULT_FAIL = "FAIL"
RESULT_ABORT = "ABORT"
VALID_RESULTS = frozenset((RESULT_PASS, RESULT_FAIL, RESULT_ABORT))

_REQUIRED_VERDICT_FIELDS = (
    "tree_sha",
    "base_tip",
    "branch",
    "result",
    "stage",
    "gate_version",
    "rustc_id",
    "platform_id",
    "host",
    "duration_s",
    "write_set",
)

# OPTIONAL fields a gate may add. Listed here (not enforced) so the payload
# contract is readable in one place; `validate_payload` tolerates any extra
# key, which is what lets a newer gate.sh record more without every reader
# being updated first.
#
#   fleet_tests_flakes -- [{"module": str, "failure": str}, ...]
#       Written by gate.sh (GATE_VERSION >= 7) when the fleet-tests stage
#       went red and EVERY failing module then passed in isolation: the
#       stage passes, and this records what flaked so burn-in can measure
#       the real rate off this cache instead of guessing. ABSENT, never
#       `[]`, on runs with no flake -- including every run that failed
#       before reaching the stage -- so "no flakes" and "this gate version
#       does not record flakes" stay distinguishable. See gate.sh's
#       BLOCKER A header comment for the measurement and the policy.
#
# Deliberately a comment and not a validated tuple: an optional field that
# `store()` could REJECT would turn a malformed extra key into "a PASSing
# gate's verdict never reached the cache", which is a worse failure than
# the sloppy telemetry it would be guarding against.


# --------------------------------------------------------------------- #
# Toolchain identity
# --------------------------------------------------------------------- #


def compute_ids(rustc_vv_text: str) -> "tuple[str, str]":
    """(rustc_id, platform_id) from the text of `rustc -vV`.

    `platform_id` hashes the text verbatim. `rustc_id` hashes it with any
    line starting with `host:` removed first -- mirrors `gate.sh`'s
    `grep -v '^host:'`. Kept here as the portable reference implementation
    so any other Python fleet tool (doctor, ledger, train) that needs a
    toolchain identity computes it the same way `gate.sh` does, rather than
    reinventing the stripping rule a third time.
    """
    platform_id = hashlib.sha256(rustc_vv_text.encode("utf-8")).hexdigest()
    stripped_lines = [line for line in rustc_vv_text.splitlines() if not line.startswith("host:")]
    stripped_text = "\n".join(stripped_lines)
    if rustc_vv_text.endswith("\n") and stripped_text:
        stripped_text += "\n"
    rustc_id = hashlib.sha256(stripped_text.encode("utf-8")).hexdigest()
    return rustc_id, platform_id


# --------------------------------------------------------------------- #
# Verdict payload
# --------------------------------------------------------------------- #


def validate_payload(payload: dict) -> List[str]:
    """Field-level problems with a verdict payload; empty list == valid.

    Never raises -- callers decide whether a problem is fatal. `store()`
    does refuse to write an invalid payload; `lookup` tolerates missing
    optional context and reports a specific rejection reason instead of
    crashing on a KeyError.
    """
    problems = []
    for field in _REQUIRED_VERDICT_FIELDS:
        if field not in payload:
            problems.append(f"missing field {field!r}")
    result = payload.get("result")
    if result is not None and result not in VALID_RESULTS:
        problems.append(f"result {result!r} not one of {sorted(VALID_RESULTS)}")
    write_set = payload.get("write_set")
    if write_set is not None and not isinstance(write_set, list):
        problems.append("write_set must be a list")
    return problems


def verdict_ref(tree_sha: str, gate_version: str, platform_id: str) -> str:
    """The hub ref a verdict lives at.

    The base namespace in FLEET_SPEC.md §3 is `refs/fleet/verdicts/<tree-sha>`;
    the cache key per M6 is `(tree_sha, gate_version, platform_id)`, so the
    ref is that namespace with the other two key components appended as
    further path segments -- still "one ref per key" (design principle 3),
    just a three-part key instead of one.
    """
    for name, value in (("tree_sha", tree_sha), ("gate_version", gate_version), ("platform_id", platform_id)):
        if not value or "/" in value or value in (".", ".."):
            raise ValueError(f"invalid {name} for a verdict ref: {value!r}")
    return f"refs/fleet/verdicts/{tree_sha}/{gate_version}/{platform_id}"


# --------------------------------------------------------------------- #
# Cache: lookup / store
# --------------------------------------------------------------------- #


def lookup(hub: Hub, tree_sha: str, gate_version: str, platform_id: str) -> Optional[dict]:
    """The cached verdict for this key, or None if there isn't an
    admissible one to reuse.

    ABORT is deliberately never returned here even if one is on the hub:
    an ABORT verdict schedules a retry, it is not this tree's settled
    answer, so a lookup must behave exactly like a cache miss and let the
    caller re-run the gate.
    """
    ref = verdict_ref(tree_sha, gate_version, platform_id)
    payload = hub.read(ref)
    if payload is None:
        return None
    if payload.get("result") == RESULT_ABORT:
        return None
    return payload


def store(hub: Hub, payload: dict, max_attempts: int = 5) -> str:
    """Write `payload` to its cache slot, honouring ABORT's retry contract.

    Returns one of:
      "created"       -- this was the first verdict for this key.
      "retried-abort" -- a prior ABORT at this key was replaced by a real
                          (PASS/FAIL) result -- the retry ABORT promised.
      "cache-hit"      -- an identical result was already there; nothing
                          written (the whole point of content-addressing).
      "conflict"       -- a *different* non-ABORT result already sits at
                          this key. Refused, not overwritten -- same
                          doctrine as never approximating a conversion:
                          report and count a contradiction, never guess
                          which one wins. This means the gate was not
                          deterministic for this (tree, gate_version,
                          platform) triple, which is worth investigating.
    """
    problems = validate_payload(payload)
    if problems:
        raise ValueError(f"refusing to store an invalid verdict payload: {'; '.join(problems)}")

    ref = verdict_ref(payload["tree_sha"], payload["gate_version"], payload["platform_id"])

    for _ in range(max_attempts):
        existing_sha = hub.sha(ref)
        if existing_sha is None:
            if hub.create(ref, payload):
                return "created"
            continue  # lost the create race; re-resolve and try again

        existing = hub.read(ref)
        if existing is None:
            continue  # deleted between sha() and read(); slot is free again

        if existing.get("result") == RESULT_ABORT and payload.get("result") != RESULT_ABORT:
            if hub.update(ref, payload, existing_sha):
                return "retried-abort"
            continue  # lost the update race; re-resolve and try again

        if existing.get("result") == payload.get("result"):
            return "cache-hit"

        return "conflict"

    raise HubError(f"store() did not converge on {ref} after {max_attempts} attempts")


# --------------------------------------------------------------------- #
# CLI -- the interface `gate.sh` shells out to
# --------------------------------------------------------------------- #


def _read_token_file(path: Optional[str]) -> Optional[str]:
    """The bearer token in `path`, stripped, or None. Duplicates
    `keel.cli._read_token_file`'s eight lines rather than importing
    `keel.cli`: that module also imports `workqueue` and the full `keel`
    operator-CLI surface, which is the wrong thing to pull into a lean,
    gate.sh-invoked module for the sake of one helper. Never raises --
    a token file this script cannot read is reported and treated as "no
    token", matching `_cli_store`'s own best-effort posture."""
    if not path:
        return None
    try:
        text = Path(path).expanduser().read_text().strip()
    except OSError as exc:
        print(f"verdict.py: warning: could not read token file {path}: {exc}", file=sys.stderr)
        return None
    return text or None


def build_hub(args: argparse.Namespace):
    """The `Hub`-shaped object every verdict operation reads/writes
    through: a plain `fleetlib.Hub` when no server is configured (today's
    exact behaviour, byte for byte -- every existing `gate.sh` invocation
    never passes `--server-url` and must keep working unchanged), or
    `FallbackHub(ServerHub, Hub)` when one is (PLAN Stage 3 task 7: "a
    gate can store its verdict through the server, falling back to
    direct" -- SPEC SS4.3's two rules apply unmodified, since this is the
    identical `FallbackHub` every other coordination write goes through).

    `--server-url` with no `--hub-url` still requires `--hub-url`
    (`argparse` already enforces it as `required=True`): the server is an
    accelerant for the SAME state repo, never a replacement for knowing
    where it is -- `FallbackHub` needs its GitHub half regardless of
    whether the primary ever gets used.
    """
    github = Hub(url=args.hub_url, workdir=Path(args.workdir))
    server_url = getattr(args, "server_url", None) or os.environ.get("KEEL_SERVER_URL")
    if not server_url:
        return github
    token = _read_token_file(getattr(args, "token_file", None) or os.environ.get("KEEL_TOKEN_FILE"))
    primary = ServerHub(server_url, token=token)
    return FallbackHub(primary, github)


def _add_server_args(p: argparse.ArgumentParser) -> None:
    p.add_argument(
        "--server-url", dest="server_url", default=None,
        help="keel-server base URL (or KEEL_SERVER_URL); when set, this operation is "
        "attempted through the server first and falls back to the hub directly on an "
        "unreachable/before-send failure (SPEC SS4.3). Omit to talk to the hub directly, "
        "exactly as before this flag existed.",
    )
    p.add_argument(
        "--token-file", dest="token_file", default=None,
        help="file holding the server bearer token (or KEEL_TOKEN_FILE); ignored when "
        "--server-url is not set.",
    )


def _cli_lookup(args: argparse.Namespace) -> int:
    hub = build_hub(args)
    try:
        payload = lookup(hub, args.tree_sha, args.gate_version, args.platform_id)
    except HubUnreachableError as exc:
        print(f"verdict.py: hub unreachable, treating as cache miss: {exc}", file=sys.stderr)
        return 2
    if payload is None:
        return 1
    print(json.dumps(payload))
    return 0


def _cli_store(args: argparse.Namespace) -> int:
    payload = json.loads(Path(args.json_file).read_text(encoding="utf-8"))
    hub = build_hub(args)
    try:
        outcome = store(hub, payload)
    except HubUnreachableError as exc:
        print(f"verdict.py: hub unreachable, verdict not cached: {exc}", file=sys.stderr)
        return 0  # best-effort: never fail the gate over a cache-store problem
    print(outcome)
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    lookup_p = sub.add_parser("lookup", help="print a cached verdict if one admits this key, else exit 1")
    lookup_p.add_argument("--hub-url", required=True)
    lookup_p.add_argument("--workdir", required=True)
    lookup_p.add_argument("--tree-sha", required=True)
    lookup_p.add_argument("--gate-version", required=True)
    lookup_p.add_argument("--platform-id", required=True)
    _add_server_args(lookup_p)
    lookup_p.set_defaults(func=_cli_lookup)

    store_p = sub.add_parser("store", help="write a verdict JSON file to its cache slot")
    store_p.add_argument("--hub-url", required=True)
    store_p.add_argument("--workdir", required=True)
    store_p.add_argument("--json-file", required=True)
    _add_server_args(store_p)
    store_p.set_defaults(func=_cli_store)

    ns = parser.parse_args(argv)
    return ns.func(ns)


if __name__ == "__main__":
    raise SystemExit(main())
