#!/usr/bin/env python3
"""Intent registry -- T1.4 (`docs/FLEET.md` P6, M5 "Intent registry and
duplicate detection").

This is the fix for the `solo-ryzen5` incident: an agent spent hours
writing `src/parsers/specialized/legacy.rs` -- 478 lines routing SWF, PICT,
PPM, RA and Kyocera RAW -- for formats the tip had *already* routed as
`5cef5b3d` (1,493 lines across five per-format files, measured at 97.3%).
Nothing detected the duplication until a human diffed it. Separately, a
queue of 35 branches turned out to be ~10 distinct items in 35 copies.

`register()` writes `refs/fleet/intents/<slug>` -- existence there is the
claim, via `fleetlib.Hub.create`'s ref-level compare-and-swap, the same
atomic primitive `claim.py` builds leases on -- and refuses on any of three
independent checks:

1. **Open-intent overlap** (`check_open_intent_overlap`) -- does another
   *open* intent already share a format, tag, or file glob? Cheap dedup:
   "is somebody already claiming to work on this."
2. **History** (`check_history`) -- does a commit message on the tip
   already mention these scope tokens? Cheap dedup: "does the commit log
   say this already happened." A text match, nothing more -- it is not
   asked to prove anything, and by itself it would not have been a
   trustworthy fix for the `solo-ryzen5` incident (an agent that read a
   commit message and decided it didn't really apply is exactly how this
   happened the first time).
3. **Capability ledger** (`ledger.check_scope`) -- **the check that
   matters.** Builds and runs the real binary at the tip against the real
   pinned ExifTool oracle on a real corpus sample and asks what it actually
   does, never what a commit message claims. See `ledger.py`'s module
   docstring for the full rationale and the traps it avoids.

All three checks always run (no short-circuiting on the first hit) so a
refusal's evidence is never partial. When more than one check fires, the
refusal's primary `reason` is chosen by priority
**capability-ledger > history > open-intent-overlap** -- specifically so
that a case like `solo-ryzen5`, where the commit message *also* happens to
mention the right words, is reported by way of its measured evidence
("compared N tags, MISSING 0") rather than the coincidence that the words
matched. `RegisterResult.checks` always carries every check's own result,
so nothing is hidden even when it isn't the headline reason.

Standard library only.
"""

from __future__ import annotations

import re
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

import ledger  # noqa: E402
from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402

INTENTS_PREFIX = "refs/fleet/intents"
DEFAULT_TIP = "refactor/tag-machinery"


class IntentError(HubError):
    """Base class for intent.py-specific errors."""


def intent_ref(slug: str) -> str:
    if "/" in slug or not slug:
        raise ValueError(f"intent slug must be non-empty and contain no '/': {slug!r}")
    return f"{INTENTS_PREFIX}/{slug}"


def _utcnow_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


# --------------------------------------------------------------------- #
# Check 1: open-intent overlap
# --------------------------------------------------------------------- #


@dataclass
class CheckResult:
    name: str
    hit: bool
    detail: str


def _norm_tokens(values) -> set:
    return {re.sub(r"[^A-Za-z0-9]", "", v).upper() for v in (values or []) if v}


def _glob_prefix(pattern: str) -> str:
    """The static (non-wildcard) leading portion of a glob, up to the last
    `/` before the first wildcard character. `"src/parsers/**"` ->
    `"src/parsers/"`; `"src/core/format_dispatch.rs"` -> itself (no
    wildcard at all, so the whole path is the "prefix").
    """
    m = re.search(r"[*?\[]", pattern)
    if m is None:
        return pattern
    head = pattern[: m.start()]
    return head.rsplit("/", 1)[0] + "/" if "/" in head else ""


def _files_overlap(a, b) -> bool:
    """Two file-glob lists "overlap" if either's static prefix is a
    path-component-wise prefix of the other's (in either direction), or the
    patterns are identical. This is a heuristic dedup signal, not a proof --
    same spirit as `fleet/domains.toml`'s conflict domains being a
    hand-maintained approximation of "these two changes might collide."
    """
    for pa in a or []:
        for pb in b or []:
            if pa == pb:
                return True
            prefix_a, prefix_b = _glob_prefix(pa), _glob_prefix(pb)
            if not prefix_a or not prefix_b:
                continue
            if prefix_a.startswith(prefix_b) or prefix_b.startswith(prefix_a):
                return True
    return False


def _scopes_overlap(a: dict, b: dict) -> Optional[str]:
    fmt = _norm_tokens(a.get("formats")) & _norm_tokens(b.get("formats"))
    if fmt:
        return f"formats {sorted(fmt)}"
    tags = _norm_tokens(a.get("tags")) & _norm_tokens(b.get("tags"))
    if tags:
        return f"tags {sorted(tags)}"
    if _files_overlap(a.get("files"), b.get("files")):
        return f"file globs {a.get('files')!r} vs {b.get('files')!r}"
    return None


def list_open_intents(hub: Hub) -> dict:
    """{slug: payload} for every intent on the hub whose status is "open"."""
    refs = hub.list(INTENTS_PREFIX)
    out = {}
    for ref in refs:
        slug = ref[len(INTENTS_PREFIX) + 1 :]
        payload = hub.read(ref)
        if payload and payload.get("status") == "open":
            out[slug] = payload
    return out


def check_open_intent_overlap(hub: Hub, slug: str, scope: dict) -> CheckResult:
    try:
        open_intents = list_open_intents(hub)
    except HubUnreachableError as exc:
        raise IntentError(f"could not list open intents: {exc}") from exc
    for other_slug, payload in open_intents.items():
        if other_slug == slug:
            continue
        overlap = _scopes_overlap(scope, payload.get("scope", {}))
        if overlap:
            return CheckResult(
                "open-intent-overlap",
                True,
                f"open intent {other_slug!r} (claimed_by={payload.get('claimed_by')!r}) "
                f"already covers {overlap}",
            )
    return CheckResult("open-intent-overlap", False, f"no overlap with {len(open_intents)} open intent(s)")


# --------------------------------------------------------------------- #
# Check 2: history (commit-message text match on the tip)
# --------------------------------------------------------------------- #


def _git_log_grep(repo_root: Path, tip: str, token: str, timeout: int = 20) -> list:
    """Oneline `sha subject` for every commit reachable from `tip` whose
    full message (subject + body) mentions `token`, case-insensitive.
    """
    try:
        out = subprocess.run(
            ["git", "-C", str(repo_root), "log", "--oneline", "-i", f"--grep={token}", tip],
            capture_output=True,
            timeout=timeout,
        )  # nosec B603
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise IntentError(f"git log --grep={token!r} {tip!r} failed: {exc}") from exc
    if out.returncode != 0:
        stderr = out.stderr.decode("utf-8", "replace").strip()
        raise IntentError(f"git log --grep={token!r} {tip!r} exited {out.returncode}: {stderr}")
    text = out.stdout.decode("utf-8", "replace").strip()
    return text.splitlines() if text else []


def _resolve_tip(repo_root: Path, tip: str, timeout: int = 10) -> str:
    """Try `tip` as given, then `origin/<tip>`, then bare `HEAD` -- a dev
    checkout may have the branch only as a remote-tracking ref.
    """
    for candidate in (tip, f"origin/{tip}", "HEAD"):
        out = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "--verify", "--quiet", candidate],
            capture_output=True,
            timeout=timeout,
        )  # nosec B603
        if out.returncode == 0:
            return candidate
    raise IntentError(f"could not resolve tip {tip!r} (nor origin/{tip!r} nor HEAD) in {repo_root}")


def check_history(repo_root: Path, scope: dict, tip: str = DEFAULT_TIP) -> CheckResult:
    resolved_tip = _resolve_tip(repo_root, tip)
    tokens = list(scope.get("formats") or []) + list(scope.get("tags") or [])
    hits = {}
    for token in tokens:
        token = token.strip()
        if not token:
            continue
        lines = _git_log_grep(repo_root, resolved_tip, token)
        if lines:
            hits[token] = lines[:5]
    if hits:
        detail = "; ".join(f"{tok!r} -> {lines[0]}" + (f" (+{len(lines) - 1} more)" if len(lines) > 1 else "") for tok, lines in hits.items())
        return CheckResult("history", True, f"commit message(s) on {resolved_tip} already mention: {detail}")
    return CheckResult("history", False, f"no commit message on {resolved_tip} mentions any scope token")


# --------------------------------------------------------------------- #
# Check 3: capability ledger -- the strong check
# --------------------------------------------------------------------- #


def check_capability_ledger(repo_root: Path, scope: dict, ledger_kwargs: Optional[dict] = None) -> CheckResult:
    """Wraps `ledger.check_scope`. A broken instrument (`ledger.LedgerError`
    -- an unusable oracle, a missing binary) is reported as a HIT, not a
    pass-through: per `ledger.py`'s doctrine, "coverage unknown" must never
    be treated as "not covered," because that is precisely how a duplicate
    would sail through registration on a day the oracle happens to be
    degraded. Failing the whole registration closed until the instrument is
    fixed is the safer failure direction.
    """
    if not scope.get("formats") and not scope.get("tags"):
        return CheckResult("capability-ledger", False, "scope names no formats or tags to measure")
    kwargs = dict(ledger_kwargs or {})
    try:
        report = ledger.check_scope(repo_root, scope, **kwargs)
    except ledger.LedgerError as exc:
        return CheckResult(
            "capability-ledger",
            True,
            f"capability ledger is unusable, refusing closed rather than guessing: {exc}",
        )
    if report.already_covered:
        return CheckResult(
            "capability-ledger",
            True,
            "capability ledger: already covered at the tip -- " + " | ".join(report.covered_reasons()),
        )
    return CheckResult(
        "capability-ledger",
        False,
        "capability ledger: not already covered -- " + (" | ".join(report.all_reasons()) or "empty scope"),
    )


# --------------------------------------------------------------------- #
# register()
# --------------------------------------------------------------------- #

_PRIORITY = {"capability-ledger": 0, "history": 1, "open-intent-overlap": 2}


@dataclass
class RegisterResult:
    ok: bool
    slug: str
    ref: str
    reason: Optional[str]
    checks: list = field(default_factory=list)  # list[CheckResult]
    payload: Optional[dict] = None


def register(
    hub: Hub,
    repo_root: Path,
    slug: str,
    title: str,
    scope: dict,
    claimed_by: str,
    tip: str = DEFAULT_TIP,
    ledger_kwargs: Optional[dict] = None,
) -> RegisterResult:
    """Register a new intent, refusing on open-intent overlap, history, or
    the capability ledger. See the module docstring for check semantics and
    refusal priority.

    Returns `RegisterResult`; never raises for a refusal (that is the
    expected, common outcome this function exists to produce) -- only
    `IntentError`/`HubUnreachableError` for genuine infrastructure failure
    (can't reach the hub, can't run git).
    """
    ref = intent_ref(slug)
    scope = {
        "formats": list(scope.get("formats") or []),
        "tags": list(scope.get("tags") or []),
        "files": list(scope.get("files") or []),
    }

    checks = [
        check_open_intent_overlap(hub, slug, scope),
        check_history(Path(repo_root), scope, tip=tip),
        check_capability_ledger(Path(repo_root), scope, ledger_kwargs=ledger_kwargs),
    ]

    hit_checks = [c for c in checks if c.hit]

    # A history hit is TEXT-MATCHING; the capability ledger is MEASUREMENT.
    # When the ledger affirmatively measured the scope as NOT covered, a
    # commit merely mentioning a scope token must not veto registration --
    # in a mature repo every format name appears in some commit message
    # (first false-refusal: "CRW" matched a Canon AFInfo commit while the
    # ledger had just measured CRW at MISSING 150/159). History still
    # refuses when the ledger could not measure (fail-closed stands).
    ledger = next((c for c in checks if c.name == "capability-ledger"), None)
    ledger_measured_open = (
        ledger is not None and not ledger.hit and "not already covered" in (ledger.detail or "")
    )
    if ledger_measured_open:
        hit_checks = [c for c in hit_checks if c.name != "history"]

    if hit_checks:
        primary = min(hit_checks, key=lambda c: _PRIORITY.get(c.name, 99))
        return RegisterResult(
            ok=False,
            slug=slug,
            ref=ref,
            reason=f"[{primary.name}] {primary.detail}",
            checks=checks,
        )

    payload = {
        "slug": slug,
        "title": title,
        "scope": scope,
        "status": "open",
        "claimed_by": claimed_by,
        "created_at": _utcnow_iso(),
    }
    try:
        created = hub.create(ref, payload)
    except HubUnreachableError:
        raise
    if not created:
        return RegisterResult(
            ok=False,
            slug=slug,
            ref=ref,
            reason=f"[slug-exists] {ref} already exists -- lost the registration race (or a stale caller retried)",
            checks=checks,
        )
    return RegisterResult(ok=True, slug=slug, ref=ref, reason=None, checks=checks, payload=payload)


def withdraw(hub: Hub, slug: str, expect_sha: Optional[str] = None) -> bool:
    """Mark an intent withdrawn (status: "withdrawn") rather than deleting
    the ref -- `queue.py` (T1.1) treats a withdrawn intent's branch as
    absent from the queue, but the record itself stays, same spirit as
    `refs/rescued/*` never being auto-deleted.
    """
    ref = intent_ref(slug)
    sha = expect_sha or hub.sha(ref)
    if sha is None:
        return False
    payload = hub.read(ref)
    if payload is None:
        return False
    payload["status"] = "withdrawn"
    return hub.update(ref, payload, expect_sha=sha)


# --------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------- #


def _main(argv=None) -> int:
    import argparse
    import getpass
    import json
    import socket

    parser = argparse.ArgumentParser(description="Register or inspect fleet intents.")
    parser.add_argument("--hub", required=True, help="hub URL (ssh://, file://, or a local path)")
    parser.add_argument("--workdir", default=None, help="local CAS cache dir (default: a temp dir)")
    parser.add_argument("--repo", default=".", help="source repo root for history/ledger checks")
    parser.add_argument("--tip", default=DEFAULT_TIP)
    sub = parser.add_subparsers(dest="cmd", required=True)

    reg = sub.add_parser("register")
    reg.add_argument("slug")
    reg.add_argument("--title", required=True)
    reg.add_argument("--format", action="append", default=[], dest="formats")
    reg.add_argument("--tag", action="append", default=[], dest="tags")
    reg.add_argument("--file", action="append", default=[], dest="files")
    reg.add_argument("--claimed-by", default=None)

    sub.add_parser("list")

    args = parser.parse_args(argv)

    import tempfile

    workdir = Path(args.workdir) if args.workdir else Path(tempfile.mkdtemp(prefix="intent-cli-"))
    hub = Hub(url=args.hub, workdir=workdir)
    repo_root = Path(args.repo).resolve()

    if args.cmd == "list":
        for slug, payload in list_open_intents(hub).items():
            print(f"{slug}\t{payload.get('title')!r}\t{payload.get('scope')}")
        return 0

    if args.cmd == "register":
        claimed_by = args.claimed_by or f"{getpass.getuser()}@{socket.gethostname()}"
        scope = {"formats": args.formats, "tags": args.tags, "files": args.files}
        result = register(hub, repo_root, args.slug, args.title, scope, claimed_by, tip=args.tip)
        print(json.dumps({"ok": result.ok, "reason": result.reason, "ref": result.ref}, indent=2))
        for c in result.checks:
            print(f"  [{'HIT ' if c.hit else 'ok  '}{c.name}] {c.detail}", file=sys.stderr)
        return 0 if result.ok else 1

    return 2


if __name__ == "__main__":
    sys.exit(_main())
