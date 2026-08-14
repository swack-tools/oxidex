#!/usr/bin/env python3
"""Verdict cache and merge-result admissibility (FLEET_SPEC.md M6, T1.2).

This is the phase that closes the one *correctness* hole in the fleet plan
(FLEET_SPEC.md §5: "P2 ... is what stops a wrong-but-green merge reaching
the tip"). Two things live here:

1. A content-addressed cache for gate verdicts, keyed by
   `(merge-result tree sha, gate_version, platform_id)`. Two hosts that
   happen to compute the same merge derive the same key, so the second one
   reuses the first's verdict instead of re-running a 20-45 minute gate.

2. `is_admissible(verdict, current_tip, ...)`: whether a *cached* verdict
   still speaks for the tree that would exist if its branch were merged to
   `current_tip` right now. A verdict about `tip@T0 + branch` is not
   automatically a verdict about `tip@T5 + branch` -- something could have
   landed on the tip in between that breaks the combination even though
   neither side touched the other's files. That is exactly what happened
   three times already (see `fleet/domains.toml`'s docstring and
   `tools/fleet/tests/test_verdict_fixtures.py`, which replays each
   incident as a fixture and asserts this function rejects it).

Two identity fields travel with every verdict, per the T0.1/T0.3 toolchain
skew this plan resolves (FLEET_SPEC.md §7 addenda, "Two tasks computed
`toolchain_id` differently, and both were right"):

  * `rustc_id`    = sha256(`rustc -vV`) with the `host:` line stripped.
                    "Is this host on the canonical compiler?"
  * `platform_id` = sha256(`rustc -vV`) unstripped. "Is this verdict
                    transferable to that host?" Part of the cache key and
                    of `is_admissible`'s platform check -- collapsing the
                    two would let a Linux PASS on `ffi_c_integration`
                    silently satisfy a macOS gate slot, which is the exact
                    cross-platform skew that cost a day.

Standard library only. `Hub` (imported from `fleetlib`, T0.2's contract --
not redefined here) is the only thing that ever talks to a remote; the
admissibility side only ever runs `git` against a local repo path that the
caller supplies.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import FrozenSet, Iterable, List, Optional, Union

sys.path.insert(0, str(Path(__file__).resolve().parent))

from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402

SCHEMA_VERSION = 1

# result: PASS | FAIL | ABORT (FLEET_SPEC.md §7 addenda).
#
# ABORT covers infrastructure failure that says nothing about the branch's
# correctness -- OOM, low disk, a lost/degraded oracle, a killed process --
# and is deliberately kept out of both `PASS` and `FAIL`'s consequences:
# `is_admissible` never admits it (same as FAIL), but the cache never
# *serves* one as a settled answer either (unlike a real FAIL), so it
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
# Conflict domains
# --------------------------------------------------------------------- #


def load_domains(path: Union[str, Path]) -> FrozenSet[str]:
    """Parse `fleet/domains.toml`'s `domains = [...]` string array.

    Deliberately not a general TOML reader -- just enough to parse this
    repo's own file (a single top-level array of quoted strings, `#`
    comments allowed after each entry), so this module needs no TOML
    dependency beyond the standard library.
    """
    text = Path(path).read_text(encoding="utf-8")
    # Anchored to the start of a line (not just a substring search) so a
    # key like `not_domains = [...]` -- which contains "domains" but is a
    # different key -- is never mistaken for the one this file declares.
    match = re.search(r"^domains\s*=\s*\[(.*?)\]", text, re.DOTALL | re.MULTILINE)
    if not match:
        raise ValueError(f"{path}: no `domains = [...]` array found")
    entries: List[str] = []
    for raw_line in match.group(1).splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        for item in line.split(","):
            item = item.strip()
            if not item:
                continue
            if not (item.startswith('"') and item.endswith('"') and len(item) >= 2):
                raise ValueError(f"{path}: unquoted or malformed domain entry {item!r}")
            entries.append(item[1:-1])
    return frozenset(entries)


# --------------------------------------------------------------------- #
# Verdict payload
# --------------------------------------------------------------------- #


def validate_payload(payload: dict) -> List[str]:
    """Field-level problems with a verdict payload; empty list == valid.

    Never raises -- callers decide whether a problem is fatal. `store()`
    does refuse to write an invalid payload; `is_admissible` and `lookup`
    tolerate missing optional context and report a specific rejection
    reason instead of crashing on a KeyError.
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
# Admissibility
# --------------------------------------------------------------------- #


class GitRepo:
    """Thin wrapper over `git` plumbing against a local repo path.

    Never talks to a remote -- `is_admissible` only needs history that is
    already present locally (the clone the gate itself made, or a synthetic
    fixture repo in tests).
    """

    def __init__(self, path: Union[str, Path]):
        self.path = str(path)

    def _git(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(["git", "-C", self.path, *args], capture_output=True, text=True)

    def is_ancestor(self, ancestor: str, descendant: str) -> bool:
        return self._git("merge-base", "--is-ancestor", ancestor, descendant).returncode == 0

    def commits_between(self, base: str, tip: str) -> List[str]:
        result = self._git("rev-list", f"{base}..{tip}")
        if result.returncode != 0:
            raise ValueError(f"git rev-list {base}..{tip} failed: {result.stderr.strip()}")
        return [line for line in result.stdout.splitlines() if line]

    def files_touched(self, commits: Iterable[str]) -> FrozenSet[str]:
        files: set = set()
        for commit in commits:
            result = self._git("diff-tree", "--no-commit-id", "--name-only", "-r", commit)
            if result.returncode != 0:
                raise ValueError(f"git diff-tree {commit} failed: {result.stderr.strip()}")
            files.update(line for line in result.stdout.splitlines() if line)
        return frozenset(files)


@dataclass(frozen=True)
class AdmissibilityResult:
    admissible: bool
    reason: str
    detail: str = ""

    def __bool__(self) -> bool:
        return self.admissible

    def __str__(self) -> str:
        return f"{self.reason}: {self.detail}" if self.detail else self.reason


def is_admissible(
    verdict: dict,
    current_tip: str,
    *,
    repo: Union["GitRepo", str, Path],
    target_platform_id: str,
    domains: Union[FrozenSet[str], Iterable[str], str, Path],
) -> AdmissibilityResult:
    """Whether `verdict` still admits its branch to `current_tip`.

    True only if, per FLEET_SPEC.md M6 / the T1.2 brief:
      1. `verdict['base_tip']` is an ancestor of `current_tip`.
      2. no commit between them touches a path in the verdict's `write_set`.
      3. neither the write_set nor those intervening commits touch a
         declared conflict domain (`fleet/domains.toml`).
      4. `verdict['result'] == 'PASS'` (FAIL and ABORT are both refused,
         though for different reasons -- see the module docstring).
      5. `verdict['platform_id'] == target_platform_id`.

    `repo` may be a `GitRepo` or a path (wrapped automatically); `domains`
    may be a pre-loaded set or a path to `domains.toml` (loaded
    automatically). This lets production code pass real paths and tests
    pass an already-built fixture without either side needing to know the
    other's convention.
    """
    if not isinstance(repo, GitRepo):
        repo = GitRepo(repo)
    if isinstance(domains, (str, Path)):
        domains = load_domains(domains)
    elif not isinstance(domains, frozenset):
        domains = frozenset(domains)

    result = verdict.get("result")
    if result != RESULT_PASS:
        return AdmissibilityResult(False, "not-pass", f"result={result!r}")

    if verdict.get("platform_id") != target_platform_id:
        return AdmissibilityResult(
            False,
            "platform-mismatch",
            f"verdict platform_id={verdict.get('platform_id')!r} != target {target_platform_id!r}",
        )

    base_tip = verdict.get("base_tip")
    if not base_tip:
        return AdmissibilityResult(False, "no-base-tip", "verdict carries no base_tip")

    write_set = frozenset(verdict.get("write_set") or [])
    branch_domain_hit = write_set & domains
    if branch_domain_hit:
        return AdmissibilityResult(False, "conflict-domain-branch", f"files={sorted(branch_domain_hit)}")

    if base_tip != current_tip:
        if not repo.is_ancestor(base_tip, current_tip):
            return AdmissibilityResult(
                False,
                "not-ancestor",
                f"base_tip {base_tip} is not an ancestor of current_tip {current_tip}",
            )

        intervening = repo.commits_between(base_tip, current_tip)
        touched = repo.files_touched(intervening)

        write_overlap = touched & write_set
        if write_overlap:
            return AdmissibilityResult(False, "write-set-overlap", f"files={sorted(write_overlap)}")

        domain_hit = touched & domains
        if domain_hit:
            return AdmissibilityResult(False, "conflict-domain-intervening", f"files={sorted(domain_hit)}")

    return AdmissibilityResult(True, "ok")


# --------------------------------------------------------------------- #
# CLI -- the interface `gate.sh` shells out to
# --------------------------------------------------------------------- #


def _cli_lookup(args: argparse.Namespace) -> int:
    hub = Hub(url=args.hub_url, workdir=Path(args.workdir))
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
    hub = Hub(url=args.hub_url, workdir=Path(args.workdir))
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
    lookup_p.set_defaults(func=_cli_lookup)

    store_p = sub.add_parser("store", help="write a verdict JSON file to its cache slot")
    store_p.add_argument("--hub-url", required=True)
    store_p.add_argument("--workdir", required=True)
    store_p.add_argument("--json-file", required=True)
    store_p.set_defaults(func=_cli_store)

    ns = parser.parse_args(argv)
    return ns.func(ns)


if __name__ == "__main__":
    raise SystemExit(main())
