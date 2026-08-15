#!/usr/bin/env python3
"""THE SEAM SUITE -- acceptance for the core-architecture fix
(`ARCH-FIX-SPEC.md`, "Acceptance for the whole effort").

WHY THIS FILE EXISTS, in one sentence borrowed from the review that
produced it:

    safety contracts existed in prose, primitive, and consumer, and
    nothing tested the composition at production timescale -- every
    day-one defect lived in a seam whose halves had green unit tests.

Every other test file under `tools/fleet/tests/` tests one half of one
seam, and every one of them was green on the day the fleet was found to
have never held a lease past minute ten, never excluded a branch claimed
by another host, and to be one `len()` heuristic away from pushing a tree
that had just gated FAIL. Halves are not the product. This file tests the
compositions:

  1. lease-through-work      fleetd's real reconcile loop runs a gate that
                             OUTLIVES the lease TTL, and the claim is
                             continuously live on the hub the whole way
                             through, unreapable by a third party, then
                             released, with the verdict memo written.
  2. exclusion round-trip    a claim held by host A's fleetd keeps the
                             branch out of host B's independently computed
                             queue for the full duration, INCLUDING past
                             the nominal TTL, and the branch comes back
                             after release.
  3. train end-to-end        the real `run_train` against a real fixture
                             hub: poison ejected, survivors landed, tip
                             advanced EXACTLY once, staging refs retired
                             only after a verified rescue, a second
                             concurrent run refused -- plus the structural
                             guard that a survivor set whose union FAILed
                             can never be pushed.
  4. restart adoption        SIGKILL a fleetd mid-gate; its successor on
                             the same host takes the work over without the
                             claim lapsing and without a second gate.
  5. hook enforcement        the R1 hub-side guard, installed by the REAL
                             installer, in the REAL train's push path: the
                             train's token'd push lands, a rogue tokenless
                             push to the same ref is denied, staging is
                             unaffected, and post-receive still bumps the
                             generation signal afterwards.
  6. the negative control    disable the single most load-bearing line of
                             the lease fix (the `start_renewer()` inside
                             `Claim.acquire`) and prove seams 1 and 2 go
                             RED. A suite that cannot fail is not evidence
                             of anything; this is how we know it would
                             have caught the original defect.
  7. (added BY this suite)   `fleetlib.Hub.read` resolves a ref's sha,
                             then fetches the ref, then cat-files the sha
                             it resolved first -- and raises when the ref
                             moved in between. Unreachable until R2 made
                             claims renew and R4 made the queue read every
                             claim payload; hit for real during a seam-2
                             run. Reproduced deterministically; WAS RED
                             (`expectedFailure`), now fixed and green --
                             see `TestSeam7HubReadRaceUnderRenewal`'s own
                             docstring for the fix and how it was found.

NAME THE INSTRUMENT (AGENTS.md). Liveness is always observed from a
SECOND `Hub` with its own object cache -- what another host would see --
never from the holder's own in-memory state, which is the thing that was
wrong. Queue exclusion is computed by a THIRD `Hub` under a different
`FLEET_HOST`. `setUpModule` prints an `=== instrument: ... ===` header
naming the mode, the timescale, the interpreter, git, and which seams are
skipped for a missing dependency, so a green run can never be mistaken
for a run that measured everything.

TWO MODES.

  * CI mode (default). Thirteen production minutes compressed into
    fifteen seconds via T1's `FLEET_TEST_TTL_S` / `FLEET_TEST_RENEW_S`,
    at production's own 5:1 TTL-to-renewal ratio. The compression is the
    mechanism under test, not a shortcut around it.
  * SLOW mode (`FLEET_SEAMS_SLOW=1`). The production numbers verbatim --
    600s TTL, 120s renewal -- with a single hold of thirteen minutes, which
    is longer than the defect took to appear in production. Marked, and
    run in stage-4 burn-in only; it is skipped otherwise.

RULES THIS FILE KEEPS. No Rust is ever built: every gate here is a shell
stub or a Python function. Every hub is a `git init --bare` under the
system temp dir and every `setUp` asserts that before a test body runs.
Standard library and plain unittest only. Nothing here modifies
production code -- where a seam test is red because production is wrong,
it stays red with the reason written at the assertion, carried as
`expectedFailure` (not deletion, not a skip) so unittest reports an
UNEXPECTED SUCCESS the day someone fixes it, and the marker cannot
outlive the defect.

ZERO EXPECTED FAILURES TODAY. Two tests used to carry `expectedFailure`
here -- `TestSeam3TrainEndToEnd.test_landed_branch_flips_its_intent_to_done`
(fixed by `intent.mark_done`, ARCH-FIX FIX-3a) and
`TestSeam7HubReadRaceUnderRenewal` (fixed by `fleetlib.Hub.read_with_sha`'s
fetch-first rewrite) -- and both are green, unmarked regression tests now;
see each one's own docstring for its fix. This paragraph is the reason a
reader who remembers "two expectedFailures" from an earlier pass of this
file does not go looking for decorators that are no longer here.

Run with (from the repo root):
    python3 -m unittest discover -s tools/fleet/tests -v
    python3 -m unittest discover -s tools/fleet/tests -p 'test_seams.py' -v
    FLEET_SEAMS_SLOW=1 python3 -m unittest discover -s tools/fleet/tests \\
        -p 'test_seams.py' -k thirteen_real_minutes -v
"""

from __future__ import annotations

import contextlib
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import claim as claim_mod  # noqa: E402
import fleetd  # noqa: E402
import train  # noqa: E402
import verdict as verdict_mod  # noqa: E402
import workqueue  # noqa: E402
from claim import Claim, claim_ref, is_expired, reap_expired  # noqa: E402
from fleetlib import Hub  # noqa: E402

FLEET_DIR = Path(__file__).resolve().parents[1]
FLEETD_PY = FLEET_DIR / "fleetd.py"
VERDICT_PY = FLEET_DIR / "verdict.py"
INSTALL_HOOK = FLEET_DIR / "rollout" / "install_hook.sh"

TIP_REF = "refs/heads/refactor/tag-machinery"

# --------------------------------------------------------------------- #
# Timescale
# --------------------------------------------------------------------- #

SLOW = os.environ.get("FLEET_SEAMS_SLOW") == "1"

# CI mode. The RATIOS are production's, not just smaller numbers: 5:1
# TTL-to-renewal (600:120) so a renewal has exactly as many chances to
# land here as it does in production, and a hold of 3x the TTL so the
# lease is outlived three times over. A defect that only appears after
# the TTL passes is not tested at all by a test that finishes before it.
CI_TTL_S = 5.0
CI_RENEW_S = 1.0
CI_WORK_S = 15.0

# SLOW mode. The production constants, unmodified. The hold is thirteen
# minutes -- past the ten-minute mark at which every real gate silently
# lost its lease before R2, with enough margin left after the TTL passes
# for the mid-work reap probe and several more liveness observations. A
# hold that merely grazes the TTL would prove the property only in the
# last few seconds of the run.
SLOW_TTL_S = 600.0
SLOW_RENEW_S = 120.0
SLOW_WORK_S = 780.0

FLEETD_INTERVAL_S = 1  # fleetd's own loop cadence; not the property under test


def poll_interval(ttl: float) -> float:
    """How often to ask the hub whether a lease is still live. Fine enough
    that a lapse of one TTL cannot slip between two observations, coarse
    enough not to spend the burn-in run in `ls-remote`."""
    return max(0.25, min(5.0, ttl / 8.0))


def queue_poll_interval(ttl: float) -> float:
    """`Queue.compute()` fetches every staging ref, so it is polled less
    often than a single-ref liveness read."""
    return max(1.0, min(10.0, ttl / 4.0))


# --------------------------------------------------------------------- #
# Dependency probes -- a seam whose production half has not landed yet is
# SKIPPED WITH A REASON, never silently dropped and never quietly passed.
# --------------------------------------------------------------------- #

# --------------------------------------------------------------------- #
# Tolerating a DIFFERENT, separately-pinned production defect
# --------------------------------------------------------------------- #
#
# `fleetlib.Hub.read` is a time-of-check/time-of-use race:
#
#     found_sha = self._remote_sha(ref)          # ls-remote  -> S1
#     self._run(["fetch", ..., f"+{ref}:{tmp}"]) # fetch      -> S2 (moved!)
#     self._run(["cat-file", "-p", f"{found_sha}:payload.json"])
#
# If the ref moves between the ls-remote and the fetch, the fetch brings
# the NEW commit and the cat-file asks for the OLD one, which was never
# fetched. Payload commits are orphans, so nothing else drags S1 in. Git
# then reports `fatal: path 'payload.json' does not exist in '<S1>'`
# (verified: that is what git says for an object it does not have, NOT
# "invalid object name"), and `Hub.read` turns that into a raised
# `HubError`.
#
# Before R2 no claim ref ever moved, because nothing renewed; the race was
# unreachable. R2 makes every held claim rewrite its ref every renewal
# interval, and R4 makes `workqueue.Queue.compute()` read EVERY claim
# payload on every call, so the two fixes together make a latent bug live.
# `TestSeam7HubReadRaceUnderRenewal` pins it deterministically. This suite
# hit it for real, unprompted, during a seam-2 run.
#
# The seams below observe claims and queues constantly, so they would
# otherwise be intermittently ERRORed by a defect they are not measuring.
# They retry ONLY this exact failure, record every occurrence, and print
# the tally -- the property each seam asserts is untouched, and the
# workaround is never silent.

_HUB_READ_RACE_MARKER = "has no readable payload.json"
HUB_READ_RACE_HITS: list = []


def tolerating_the_hub_read_race(call, what: str, attempts: int = 5):
    """Run `call()`, retrying only `Hub.read`'s TOCTOU failure.

    Anything else propagates untouched. See the block comment above and
    `TestSeam7HubReadRaceUnderRenewal`.
    """
    from fleetlib import HubError  # local import: same module, named here for clarity

    last = None
    for _ in range(attempts):
        try:
            return call()
        except HubError as exc:
            if _HUB_READ_RACE_MARKER not in str(exc):
                raise
            HUB_READ_RACE_HITS.append(f"{what}: {exc}")
            last = exc
            time.sleep(0.1)
    raise AssertionError(
        f"{what}: fleetlib.Hub.read's ls-remote/fetch race failed {attempts} times in a row, "
        f"which is no longer 'a narrow window' -- last error: {last}"
    )


# A claim ref read as ABSENT is confirmed before it is believed. Two
# reasons, neither of them "make the test pass":
#
#   * A real lapse persists. The host is drained before the observation
#     loop starts, so nothing recreates a claim that was genuinely
#     released or reaped -- a confirmed absence is still an absence three
#     reads later, and the assertion still fires. The negative control
#     (seam 6) is the standing proof of that: disable the renewer and this
#     same loop goes red on demand.
#   * A single `ls-remote` is not a proof of absence. One run under heavy
#     load (two sibling test suites saturating the box) read
#     `refs/fleet/claims/gate/staging-nc2` as gone 2.8s into a hold while
#     fleetd's own log showed neither a `finished` nor a `killed` for it,
#     i.e. nothing had released it. Unreproducible in 18 subsequent runs
#     on an idle machine. Whatever that was -- a transient in git's ref
#     read under a concurrent `pack-refs`, a stalled subprocess -- it is
#     not the lease property, and an acceptance harness that reds out on
#     it teaches people to re-run instead of to read.
#
# Every rescue is counted and printed, so a run that needed one is never
# reported as a clean run.
TRANSIENT_ABSENCE_HITS: list = []


def observe_claim(observer, ref: str, what: str, confirmations: int = 3, gap: float = 0.3):
    """The claim's payload, or None only after `confirmations` agree."""
    payload = tolerating_the_hub_read_race(lambda: observer.read(ref), what)
    if payload is not None:
        return payload
    for _ in range(confirmations):
        time.sleep(gap)
        again = tolerating_the_hub_read_race(lambda: observer.read(ref), what)
        if again is not None:
            TRANSIENT_ABSENCE_HITS.append(
                f"{what}: {ref} read as absent once, then present {gap:g}s later"
            )
            return again
    return None


def report_transient_absence_hits(prefix: str) -> None:
    if TRANSIENT_ABSENCE_HITS:
        print(
            f"\n  !! {prefix}: a claim ref read as ABSENT and was present again on re-read "
            f"{len(TRANSIENT_ABSENCE_HITS)} time(s). Not a lease lapse (see observe_claim), but "
            f"NOT NOTHING either -- if this number is ever more than a blip, the hub's ref reads "
            f"are not trustworthy and every consumer of Hub.sha is affected. "
            f"First: {TRANSIENT_ABSENCE_HITS[0]}",
            file=sys.stderr,
            flush=True,
        )


def report_hub_read_race_hits(prefix: str) -> None:
    if HUB_READ_RACE_HITS:
        print(
            f"\n  !! {prefix}: fleetlib.Hub.read's ls-remote/fetch race fired "
            f"{len(HUB_READ_RACE_HITS)} time(s) during this run and was retried past. "
            f"This is a REAL production defect -- see TestSeam7HubReadRaceUnderRenewal. "
            f"First: {HUB_READ_RACE_HITS[0]}",
            file=sys.stderr,
            flush=True,
        )


_ADOPTION_HINTS = ("adopt", "rebuild", "recover")


def adoption_entry_point():
    """The name of fleetd's restart-adoption entry point (R6/T6), or None.

    Probed by capability rather than by branch name: what matters is
    whether the production code can rebuild Worker state from its host's
    live claims, not which branch happened to deliver it.
    """
    import inspect

    for name in dir(fleetd):
        low = name.lower()
        if name.startswith("_"):
            continue
        obj = getattr(fleetd, name)
        # A FUNCTION, not merely something callable: `AdoptionResult` is a
        # dataclass and passes a bare `callable()` test, so a probe that
        # accepts it would report "adoption is wired" for a branch that
        # shipped only the result type.
        if any(hint in low for hint in _ADOPTION_HINTS) and inspect.isfunction(obj):
            return name
    return None


ADOPTION_SUPPORTED = adoption_entry_point() is not None

ADOPTION_SKIP_REASON = (
    "SEAM 4 NOT WIRED YET -- fleetd exposes no restart-adoption entry point "
    "(probed for a public callable whose name mentions adopt/rebuild/recover; "
    "found none). This is R6/T6 (`staging/afx-dispatch`): 'On start it rebuilds "
    "Worker state from its host's live claims + process groups.' The test body "
    "below is complete and will run the moment that lands -- it is skipped, not "
    "weakened. Until then NOTHING in this suite covers a fleetd restart, and a "
    "green run does not mean restarts are safe."
)

_QUEUE_KEY_FIX_PRESENT = None


def queue_key_fix_present() -> bool:
    """Behavioural probe for R4's claim-exclusion key fix, computed once
    against its own throwaway hub so it does not depend on test ordering.

    `fleetd.start_gate` writes `work_key="staging/<slug>"`.
    `workqueue.Queue.compute` looks that key up as the bare slug or as the
    full `refs/heads/staging/<slug>`. Those three spellings do not match,
    so before R4/T5 a claim held by one host is invisible to another
    host's queue. `test_queue.py` is green today only because it writes
    `work_key="delta"` by hand -- a unit test agreeing with itself about a
    key nothing in production writes. THAT is the seam.
    """
    global _QUEUE_KEY_FIX_PRESENT
    if _QUEUE_KEY_FIX_PRESENT is not None:
        return _QUEUE_KEY_FIX_PRESENT
    root = Path(tempfile.mkdtemp(prefix="seam-keyprobe-"))
    try:
        hub_path = str(root / "hub.git")
        git(["init", "--quiet", "--bare", hub_path])
        work = root / "seed"
        git(["init", "--quiet", str(work)])
        (work / "f.txt").write_text("f\n")
        git(["add", "."], cwd=work)
        git(["commit", "--quiet", "-m", "tip"], cwd=work)
        git(["push", "--quiet", hub_path, f"HEAD:{TIP_REF}"], cwd=work)
        # The probe branch must carry a commit the tip does NOT have.
        # Pushing the tip commit itself makes `Queue.compute` drop it as an
        # ancestor of the tip -- which looks exactly like the exclusion
        # this probe is asking about and answers the wrong question. (It
        # did, on the first draft: the probe reported the R4 fix present
        # while the seam-2 body found the branch queued 0.3s in.)
        (work / "probe.txt").write_text("probe\n")
        git(["add", "."], cwd=work)
        git(["commit", "--quiet", "-m", "keyprobe work"], cwd=work)
        git(["push", "--quiet", hub_path, "HEAD:refs/heads/staging/keyprobe"], cwd=work)

        holder = Hub(hub_path, workdir=root / "cache-holder")
        asker = Hub(hub_path, workdir=root / "cache-asker")

        # Control: with NO claim held, the branch must be queued. Without
        # this the probe cannot tell "the claim excluded it" from "it was
        # never in the queue", and a probe that cannot tell those apart
        # reports whichever answer the reader already expected.
        baseline = workqueue.Queue(asker).compute()
        if "keyprobe" not in baseline:
            raise RuntimeError(
                "seam suite's R4 key probe is broken: staging/keyprobe is not in the queue even "
                f"with no claim held (queue={sorted(baseline)}). Fix the probe before trusting "
                "any seam-2 skip or run."
            )

        held = Claim(
            holder,
            kind="gate",
            key="staging-keyprobe",
            work_kind="gate",
            work_key="staging/keyprobe",  # exactly what fleetd.start_gate writes
            ttl=600,
            renew_interval=300,
        )
        held.acquire()
        try:
            queued = workqueue.Queue(asker).compute()
        finally:
            held.release()
        _QUEUE_KEY_FIX_PRESENT = "keyprobe" not in queued
        return _QUEUE_KEY_FIX_PRESENT
    finally:
        shutil.rmtree(root, ignore_errors=True)


QUEUE_KEY_SKIP_REASON = (
    "SEAM 2 NOT WIRED YET -- R4's claim-exclusion key mismatch is still present. A live claim "
    "whose payload work_key is 'staging/keyprobe' (the exact spelling fleetd.start_gate writes) "
    "did NOT remove slug 'keyprobe' from workqueue.Queue.compute()'s result. That is finding R4 "
    "verbatim: 'The claim-exclusion key mismatch (fleetd `staging/foo` vs workqueue "
    "slug/full-ref) is fixed with a round-trip test proving a fleetd-held claim excludes its "
    "branch from another host's queue computation.' Owner: T5 (`staging/afx-queue`). The test "
    "body below is complete and unweakened; it runs the moment that lands. UNTIL THEN TWO HOSTS "
    "CAN GATE THE SAME BRANCH and nothing in this suite covers it."
)


def setUpModule():  # noqa: N802 -- unittest's spelling
    mode = "SLOW (production timescale)" if SLOW else "CI (compressed timescale)"
    ttl, renew, work = timescale()
    git_v = subprocess.run(["git", "--version"], capture_output=True, text=True).stdout.strip()
    print(
        "\n=== instrument: tools/fleet/tests/test_seams.py ===\n"
        f"  mode              : {mode}\n"
        f"  ttl/renew/work    : {ttl:g}s / {renew:g}s / {work:g}s\n"
        f"  fleetd            : {FLEETD_PY}\n"
        f"  python            : {sys.version.split()[0]} ({sys.executable})\n"
        f"  git               : {git_v}\n"
        f"  seam 4 (adoption) : {'wired via fleetd.' + str(adoption_entry_point()) if ADOPTION_SUPPORTED else 'SKIPPED -- fleetd has no restart-adoption entry point (R6/T6)'}\n"
        f"  seam 2 (exclusion): {'wired -- a fleetd-shaped claim excludes its branch' if queue_key_fix_present() else 'SKIPPED -- R4 claim-key mismatch still present (T5)'}\n"
        "===================================================",
        flush=True,
    )


def timescale():
    """(ttl, renew, work) for the current mode."""
    if SLOW:
        return SLOW_TTL_S, SLOW_RENEW_S, SLOW_WORK_S
    return CI_TTL_S, CI_RENEW_S, CI_WORK_S


# --------------------------------------------------------------------- #
# git helper
# --------------------------------------------------------------------- #

_GIT_ENV = None


def git(args, cwd=None, check=True, env_extra=None):
    global _GIT_ENV
    if _GIT_ENV is None:
        _GIT_ENV = {
            **os.environ,
            "GIT_AUTHOR_NAME": "seam",
            "GIT_AUTHOR_EMAIL": "seam@t",
            "GIT_COMMITTER_NAME": "seam",
            "GIT_COMMITTER_EMAIL": "seam@t",
            "GIT_TERMINAL_PROMPT": "0",
        }
    env = dict(_GIT_ENV)
    if env_extra:
        env.update(env_extra)
    result = subprocess.run(
        ["git"] + list(args), cwd=cwd, capture_output=True, text=True, errors="replace", env=env
    )
    if check and result.returncode != 0:
        raise AssertionError(f"git {' '.join(args)} -> {result.returncode}: {result.stderr.strip()}")
    return result


# --------------------------------------------------------------------- #
# The stub gate
# --------------------------------------------------------------------- #


class StubGate:
    """A `gate.sh` that is not a gate.

    It records that it ran, holds for `seconds` (which every caller sets
    LONGER than the lease TTL -- that is the whole experiment), writes a
    real verdict through the real `verdict.py` CLI so the memo half of the
    seam is exercised too, and exits. It builds nothing.

    `stopfile` exists so a failing test can end a hold immediately instead
    of making every red run wait out the full production timescale.
    """

    def __init__(self, root: Path, name: str, hub_url: str, seconds: float, branch: str,
                 store_verdict: bool = True):
        self.root = root
        self.name = name
        self.hub_url = hub_url
        self.seconds = seconds
        self.branch = branch
        self.runlog = root / f"{name}.runlog"
        self.stopfile = root / f"{name}.stop"
        self.repo_root = root / f"{name}-repo"
        self.tree_sha = ("seamtree" + name).replace("-", "")[:40] or "seamtree"
        self.gate_version = "seam-5"
        self.platform_id = "seamplatform" + name.replace("-", "")
        self.verdict_json = root / f"{name}.verdict.json"
        self.vcache = root / f"{name}-vcache"

        (self.repo_root / "tools" / "fleet").mkdir(parents=True, exist_ok=True)
        (self.repo_root / "tools" / "fleet" / "gate_version.txt").write_text(self.gate_version + "\n")

        payload = {
            "tree_sha": self.tree_sha,
            "base_tip": "seambasetip",
            "branch": branch,
            "result": "PASS",
            "stage": "stub",
            "gate_version": self.gate_version,
            "rustc_id": "seamrustc",
            "platform_id": self.platform_id,
            "host": "seam-host",
            "duration_s": 0,
            "write_set": [],
        }
        script = (self.repo_root / "tools" / "fleet" / "gate.sh")
        body = _STUB_GATE_TEMPLATE
        for key, value in {
            "@@RUNLOG@@": str(self.runlog),
            "@@STOPFILE@@": str(self.stopfile),
            "@@SECONDS@@": str(int(seconds)),
            "@@STORE@@": "1" if store_verdict else "0",
            "@@VERDICT_JSON@@": str(self.verdict_json),
            "@@VERDICT_PY@@": str(VERDICT_PY),
            "@@HUB@@": hub_url,
            "@@VCACHE@@": str(self.vcache),
            "@@PAYLOAD@@": json.dumps(payload, indent=2),
            "@@PYTHON@@": sys.executable,
        }.items():
            body = body.replace(key, value)
        script.write_text(body)
        script.chmod(0o755)
        self.script = script

    # -- observation ---------------------------------------------------- #

    def _log(self) -> str:
        try:
            return self.runlog.read_text()
        except OSError:
            return ""

    def starts(self) -> int:
        return len([l for l in self._log().splitlines() if l.startswith("start ")])

    def finished(self) -> bool:
        return any(l.startswith("done ") for l in self._log().splitlines())

    def stop(self) -> None:
        """End the hold now. Idempotent; safe to call on a gate that never
        started."""
        try:
            self.stopfile.write_text("stop\n")
        except OSError:
            pass


_STUB_GATE_TEMPLATE = """#!/bin/bash
# STUB GATE -- installed by tools/fleet/tests/test_seams.py. This is not a
# gate: it builds nothing, it always says PASS, and it exists only to hold
# a lease open for longer than that lease's TTL.
BRANCH="$1"
TAG="$2"
echo "start ${BRANCH} ${TAG} $$" >> "@@RUNLOG@@"
end=$(( $(date +%s) + @@SECONDS@@ ))
while [ "$(date +%s)" -lt "$end" ]; do
    if [ -f "@@STOPFILE@@" ]; then break; fi
    sleep 0.2
done
if [ "@@STORE@@" = "1" ]; then
    cat > "@@VERDICT_JSON@@" <<'SEAMJSON'
@@PAYLOAD@@
SEAMJSON
    "@@PYTHON@@" "@@VERDICT_PY@@" store --hub-url "@@HUB@@" --workdir "@@VCACHE@@" \\
        --json-file "@@VERDICT_JSON@@" >> "@@RUNLOG@@" 2>&1
fi
echo "done ${BRANCH} ${TAG}" >> "@@RUNLOG@@"
exit 0
"""


# --------------------------------------------------------------------- #
# fleetd drivers
# --------------------------------------------------------------------- #


class SubprocessFleetd:
    """The real daemon: `python3 fleetd.py --interval 1`, in its own
    session, with `HOME` redirected into the fixture so its hub cache and
    gate logs cannot touch the developer's.

    This is the default driver everywhere a seam does not need to reach
    inside the daemon, because it is the thing that actually runs in
    production -- argparse, signal handlers, singleton claim, loop and all.
    """

    kind = "subprocess"

    def __init__(self, fixture, host, gate: StubGate, ttl: float, renew: float):
        self.fixture = fixture
        self.host = host
        self.gate = gate
        self.ttl = ttl
        self.renew = renew
        self.popen = None
        self.log_path = fixture.tmp / f"fleetd-{host}.log"

    def start(self):
        env = dict(os.environ)
        env.update(
            {
                "HOME": str(self.fixture.home),
                "FLEET_HOST": self.host,
                "FLEET_TEST_TTL_S": str(self.ttl),
                "FLEET_TEST_RENEW_S": str(self.renew),
                "FLEET_KILL_GRACE_S": "2",
                "PYTHONUNBUFFERED": "1",
            }
        )
        log = open(self.log_path, "ab")
        try:
            self.popen = subprocess.Popen(
                [
                    sys.executable,
                    str(FLEETD_PY),
                    "--hub",
                    self.fixture.hub_path,
                    "--repo-root",
                    str(self.gate.repo_root),
                    "--log-dir",
                    str(self.fixture.tmp / "gatelogs"),
                    "--interval",
                    str(FLEETD_INTERVAL_S),
                ],
                stdout=log,
                stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL,
                start_new_session=True,
                env=env,
                cwd=str(self.fixture.tmp),
            )
        finally:
            log.close()
        self.fixture.track_process(self.popen)
        return self

    def alive(self) -> bool:
        return self.popen is not None and self.popen.poll() is None

    def sigkill(self):
        """Kill the DAEMON only -- not its process group. Gates are spawned
        with `start_new_session=True`, so a group kill here would take the
        gate with it and the restart seam would be testing nothing."""
        os.kill(self.popen.pid, signal.SIGKILL)
        self.popen.wait(timeout=30)

    def stop(self, timeout=30):
        if self.popen is None or self.popen.poll() is not None:
            return
        self.popen.terminate()
        try:
            self.popen.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.popen.kill()
            self.popen.wait(timeout=10)

    def log(self) -> str:
        try:
            return self.log_path.read_text(errors="replace")
        except OSError:
            return ""


class SupervisedFleetd:
    """fleetd under its REAL supervisor: `tools/fleet/units/fleetd-wrapper.sh`
    (R8), which runs `fleetd.py "$@"` in a loop and restarts it after
    `FLEETD_WRAPPER_RETRY_S` on any non-zero exit.

    Seam 4 needs this rather than a hand-started successor because
    `fleetd.main` returns 3 -- not "keeps trying" -- while another
    instance holds the host singleton, and after a SIGKILL the dead
    daemon's singleton is still on the hub. Only the retry loop ever gets
    a successor in, so only the retry loop can be said to test a restart.
    """

    kind = "supervised"

    def __init__(self, fixture, host, gate: StubGate, ttl: float, renew: float, retry_s: int = 1):
        self.fixture = fixture
        self.host = host
        self.gate = gate
        self.ttl = ttl
        self.renew = renew
        self.retry_s = retry_s
        self.popen = None
        self.out_path = fixture.tmp / f"wrapper-{host}.out"
        self.log_path = fixture.tmp / f"wrapper-{host}.log"
        self.pidfile = fixture.tmp / f"wrapper-{host}.pid"

    def start(self):
        env = dict(os.environ)
        env.update(
            {
                "HOME": str(self.fixture.home),
                "FLEET_HOST": self.host,
                "FLEET_TEST_TTL_S": str(self.ttl),
                "FLEET_TEST_RENEW_S": str(self.renew),
                "FLEET_KILL_GRACE_S": "2",
                "FLEETD_WRAPPER_RETRY_S": str(self.retry_s),
                "FLEETD_WRAPPER_PIDFILE": str(self.pidfile),
                "FLEETD_WRAPPER_LOG": str(self.log_path),
                "FLEETD_WRAPPER_PYTHON": sys.executable,
                "PYTHONUNBUFFERED": "1",
            }
        )
        out = open(self.out_path, "ab")
        try:
            self.popen = subprocess.Popen(
                [
                    "bash",
                    str(FLEET_DIR / "units" / "fleetd-wrapper.sh"),
                    "--hub", self.fixture.hub_path,
                    "--repo-root", str(self.gate.repo_root),
                    "--log-dir", str(self.fixture.tmp / "gatelogs"),
                    "--interval", str(FLEETD_INTERVAL_S),
                ],
                stdout=out,
                stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL,
                start_new_session=True,
                env=env,
                cwd=str(self.fixture.tmp),
            )
        finally:
            out.close()
        self.fixture.track_process(self.popen)
        return self

    def fleetd_pid(self):
        """The wrapper's current fleetd child, by parent pid from the `ps`
        listing -- never `pgrep -f fleetd`, which matches this test's own
        command line too (the self-match class of bug that over-reported
        gate counts on this fleet all day on 2026-08-14)."""
        if self.popen is None:
            return None
        out = subprocess.run(
            ["ps", "-eo", "pid=,ppid=,command="], capture_output=True, text=True, errors="replace"
        ).stdout
        for line in out.splitlines():
            parts = line.split(None, 2)
            if len(parts) < 3:
                continue
            pid, ppid, command = parts
            if not pid.isdigit() or not ppid.isdigit():
                continue
            if int(ppid) == self.popen.pid and "fleetd.py" in command:
                return int(pid)
        return None

    def alive(self) -> bool:
        return self.popen is not None and self.popen.poll() is None

    def stop(self, timeout=30):
        if self.popen is None or self.popen.poll() is not None:
            return
        # SIGTERM the wrapper: it forwards to fleetd, fleetd drains and
        # exits 0, the wrapper sees rc==0 and exits without restarting.
        self.popen.terminate()
        try:
            self.popen.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.popen.kill()
            self.popen.wait(timeout=10)

    def log(self) -> str:
        parts = []
        for name, path in (("wrapper stdout", self.out_path), ("wrapper log", self.log_path)):
            try:
                parts.append(f"--- {name} ---\n{path.read_text(errors='replace')}")
            except OSError:
                parts.append(f"--- {name} ---\n(unreadable)")
        return "\n".join(parts)


class InProcessFleetd:
    """`fleetd.main()`'s loop body, in a thread in THIS interpreter.

    Identical to `main()` except for argparse and the SIGTERM handler
    (`signal.signal` refuses to install off the main thread) -- same
    singleton claim, same `reconcile_once`, same drain-on-exit.

    It exists for exactly one caller: the negative control, which has to
    disable a production symbol with `mock.patch` and then watch the
    daemon behave differently. A patch in this process cannot reach the
    subprocess driver, and a negative control that cannot reach the code
    it disables proves nothing at all.
    """

    kind = "in-process"

    def __init__(self, fixture, host, gate: StubGate):
        self.fixture = fixture
        self.host = host
        self.gate = gate
        self.workers: list = []
        self.errors: list = []
        self.events: list = []
        self._stop = threading.Event()
        self._thread = None
        self.singleton = None

    def start(self):
        self._thread = threading.Thread(target=self._loop, name=f"seam-fleetd-{self.host}", daemon=True)
        self._thread.start()
        return self

    def _loop(self):
        hub = Hub(self.fixture.hub_path, workdir=self.fixture.tmp / f"cache-{self.host}")
        singleton = Claim(hub, kind="host", key=self.host, work_kind="fleetd", work_key=self.host)
        try:
            singleton.acquire_or_reap()
        except Exception as exc:  # noqa: BLE001 -- surfaced by the caller
            self.errors.append(exc)
            return
        self.singleton = singleton
        started = time.monotonic()
        try:
            while not self._stop.is_set():
                try:
                    res = fleetd.reconcile_once(
                        hub,
                        self.host,
                        self.workers,
                        [str(self.gate.script)],
                        self.fixture.tmp / "gatelogs",
                        self.gate.repo_root,
                    )
                    # The subprocess driver gets this for free from
                    # fleetd's own stdout line; record the same fields
                    # here or a failure has no way to say WHY the claim
                    # went away (reaped as finished? killed for a lost
                    # lease? two completely different bugs).
                    if res.started or res.finished or res.killed or res.refused:
                        self.events.append(
                            f"+{time.monotonic() - started:6.2f}s started={res.started} "
                            f"finished={res.finished} killed={res.killed} "
                            f"refused={res.refused}"
                        )
                except Exception as exc:  # noqa: BLE001
                    self.errors.append(exc)
                if singleton.lost:
                    self.events.append(
                        f"+{time.monotonic() - started:6.2f}s HOST LEASE LOST: "
                        f"{singleton.lost_reason}"
                    )
                    break
                self._stop.wait(FLEETD_INTERVAL_S)
        finally:
            singleton.release()

    def alive(self) -> bool:
        return self._thread is not None and self._thread.is_alive()

    def stop(self, timeout=60):
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout)
        # fleetd itself drains rather than killing on the way out; the
        # fixture is not production, so it tidies up what drain leaves.
        for w in list(self.workers):
            try:
                fleetd.kill_worker(w, grace=1)
            except Exception:  # noqa: BLE001
                pass
        self.workers.clear()

    def log(self) -> str:
        return "\n".join(
            list(self.events) + [f"{type(e).__name__}: {e}" for e in self.errors]
        )


# --------------------------------------------------------------------- #
# Fixtures
# --------------------------------------------------------------------- #


class SeamFixture(unittest.TestCase):
    """A throwaway bare hub with a seeded tip, plus the guard -- asserted
    before any test body runs -- that it is never the production hub."""

    maxDiff = None

    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="seam-")
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.tmp = Path(self._tmp_root)
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.hub_path = str(self.tmp / "hub.git")
        git(["init", "--quiet", "--bare", self.hub_path])

        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)
        self.assertNotIn("oxidex_refactor", resolved)

        # No auto-gc on the fixture hub. A compressed-timescale run pushes
        # a claim renewal and a heartbeat every second into one bare repo,
        # a rate no real hub sees, and `gc --auto` firing mid-run drags a
        # `pack-refs --prune` (and its ref-visibility window) into the
        # middle of the property being measured. Turning it off removes a
        # variable the seams are not about; it changes nothing a claim
        # does.
        git(["--git-dir", self.hub_path, "config", "gc.auto", "0"])
        git(["--git-dir", self.hub_path, "config", "receive.autogc", "false"])

        self.hub = Hub(self.hub_path, workdir=self.tmp / "cache-primary")
        self._procs: list = []
        self._gates: list = []
        self._observers = 0

        # Seed the tip. Everything after this point is a hub that looks
        # like the real one to every module under test.
        self.seed = self.tmp / "seed"
        git(["init", "--quiet", str(self.seed)])
        (self.seed / "base.txt").write_text("base\n")
        (self.seed / "fleet").mkdir()
        (self.seed / "fleet" / "domains.toml").write_text('domains = [\n  "census.rs",\n]\n')
        git(["add", "."], cwd=self.seed)
        git(["commit", "--quiet", "-m", "tip"], cwd=self.seed)
        git(["push", "--quiet", self.hub_path, f"HEAD:{TIP_REF}"], cwd=self.seed)

        # Never the developer's ~/git/oxidex.git/train.token: an unrelated
        # file on this box must not decide whether a seam exercises the
        # token path.
        self.set_env(**{train.TRAIN_TOKEN_ENV: str(self.tmp / "absent-train.token")})

    def tearDown(self):
        for gate in self._gates:
            gate.stop()
        for popen in self._procs:
            if popen.poll() is None:
                try:
                    popen.terminate()
                    popen.wait(timeout=10)
                except (OSError, subprocess.TimeoutExpired):
                    try:
                        popen.kill()
                        popen.wait(timeout=10)
                    except OSError:
                        pass
        # Give any stub gate a moment to notice its stopfile before the
        # tree it is writing into disappears underneath it.
        time.sleep(0.4)

    # -- environment ---------------------------------------------------- #

    def set_env(self, **kw):
        """Set env vars for the duration of the test, restoring them after.

        Used for the two documented compression knobs (`FLEET_TEST_TTL_S`,
        `FLEET_TEST_RENEW_S`) and for `FLEET_HOST`, which is what makes a
        second `Hub` in this process genuinely stand in for another host.
        """
        for name, value in kw.items():
            old = os.environ.get(name)
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = str(value)
            self.addCleanup(self._restore_env, name, old)

    @staticmethod
    def _restore_env(name, old):
        if old is None:
            os.environ.pop(name, None)
        else:
            os.environ[name] = old

    @contextlib.contextmanager
    def as_host(self, host):
        """Run a block as if it were `host` -- the queue is computed by
        `FLEET_HOST=<other>` in seam 2 because 'another host's queue' is
        the claim being made."""
        old = os.environ.get("FLEET_HOST")
        os.environ["FLEET_HOST"] = host
        try:
            yield
        finally:
            self._restore_env("FLEET_HOST", old)

    # -- fixture helpers ------------------------------------------------ #

    def track_process(self, popen):
        self._procs.append(popen)
        return popen

    def track_gate(self, gate: StubGate) -> StubGate:
        self._gates.append(gate)
        return gate

    def observer(self, name: str) -> Hub:
        """A second (third, fourth) `Hub` with its OWN object cache: what a
        different host sees, never the holder's own bookkeeping."""
        self._observers += 1
        return Hub(self.hub_path, workdir=self.tmp / f"cache-obs-{name}-{self._observers}")

    def tip_sha(self):
        return self.hub.sha(TIP_REF)

    def clear_staging(self):
        """Retire every staging ref on the fixture hub.

        Used between the two phases of the negative control: fleetd picks
        the FIRST admissible slug in the queue, so a leftover branch from
        the previous phase gets gated instead of the one the phase is
        about, and the seam assertions then wait forever on a claim that
        was never going to appear. (Observed exactly that; the failure
        looked like 'the fix does not work' and was 'the daemon was busy
        with the other branch'.)
        """
        for ref, sha in self.hub.list("refs/heads/staging/").items():
            self.hub.delete(ref, expect_sha=sha)

    def add_staging_branch(self, slug: str, files: dict) -> str:
        base = self.hub.sha(TIP_REF)
        git(["checkout", "--quiet", "-B", f"b-{slug}", base], cwd=self.seed)
        for name, content in files.items():
            path = self.seed / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        git(["add", "."], cwd=self.seed)
        git(["commit", "--quiet", "-m", f"work {slug}"], cwd=self.seed)
        git(["push", "--quiet", self.hub_path, f"HEAD:refs/heads/staging/{slug}"], cwd=self.seed)
        return git(["rev-parse", "HEAD"], cwd=self.seed).stdout.strip()

    def set_desired(self, host: str, gates: int = 1, agents: int = 0, enabled: bool = True):
        payload = {
            "hosts": {host: {"enabled": enabled, "gates": gates, "agents": agents}},
            "limits": {},
        }
        current = self.hub.sha(fleetd.DESIRED_REF)
        ok = (
            self.hub.create(fleetd.DESIRED_REF, payload)
            if current is None
            else self.hub.update(fleetd.DESIRED_REF, payload, expect_sha=current)
        )
        self.assertTrue(ok, "could not write the fixture's desired-state doc")

    def wait_until(self, predicate, timeout: float, what: str, poll: float = 0.25):
        start = time.monotonic()
        while time.monotonic() - start < timeout:
            if predicate():
                return time.monotonic() - start
            time.sleep(poll)
        self.fail(f"{what} did not happen within {timeout:g}s")

    def push_log_hook(self) -> Path:
        """Install a fixture `post-receive` that records every ref update.

        This is the instrument for 'the tip advanced EXACTLY once': a
        count of pushes observed by the hub itself, not an inference from
        the final sha (which cannot tell one advance from three).
        """
        log = Path(self.hub_path) / "pushes.log"
        hook = Path(self.hub_path) / "hooks" / "post-receive"
        hook.write_text(
            "#!/bin/sh\n"
            "while read -r old new ref; do\n"
            f'  echo "$ref $old $new" >> {log}\n'
            "done\n"
            "exit 0\n"
        )
        hook.chmod(0o755)
        return log

    def pushes_to(self, log: Path, ref: str) -> list:
        if not log.is_file():
            return []
        out = []
        for line in log.read_text().splitlines():
            parts = line.split()
            if len(parts) == 3 and parts[0] == ref:
                out.append((parts[1], parts[2]))
        return out


class FleetdSeamFixture(SeamFixture):
    """Adds the two composition bodies that seams 1, 2, 4 and 6 share.

    They are methods, not free functions, because the negative control has
    to run the SAME assertions against the same fixture with one
    production line disabled -- a copy would let the two drift, and a
    drifted negative control proves nothing.
    """

    HOST_A = "seam-host-a"
    HOST_B = "seam-host-b"

    def make_gate(self, name: str, branch: str, work_s: float) -> StubGate:
        return self.track_gate(StubGate(self.tmp, name, self.hub_path, work_s, branch))

    def ref_forensics(self, ref: str) -> str:
        """Ask three different instruments whether `ref` exists, right now.

        `Hub.sha` is `git ls-remote` over the hub URL; `show-ref` reads the
        bare repo's ref store directly. If they disagree, the absence the
        seam just failed on is an artifact of the reader, not a deleted
        claim -- and per AGENTS.md that has to be established before the
        number is believed, not after.
        """
        lines = []
        probe = self.observer("forensics")
        for i in range(3):
            try:
                lines.append(f"  ls-remote #{i}: {probe.sha(ref)}")
            except Exception as exc:  # noqa: BLE001
                lines.append(f"  ls-remote #{i}: raised {type(exc).__name__}: {exc}")
            time.sleep(0.2)
        direct = git(["--git-dir", self.hub_path, "show-ref", ref], check=False)
        lines.append(f"  show-ref: rc={direct.returncode} out={direct.stdout.strip()!r}")
        allrefs = git(["--git-dir", self.hub_path, "for-each-ref", "refs/fleet/claims/"], check=False)
        lines.append(f"  claims on hub: {allrefs.stdout.strip()!r}")
        return "\n--- ref forensics ---\n" + "\n".join(lines)

    @staticmethod
    def diagnosis(driver, gate: StubGate) -> str:
        """Everything needed to tell WHY a lease assertion broke, attached
        to the assertion itself.

        `fleetd` prints `finished=[...]` when it reaps a worker whose
        process is gone and `killed=[(tag, reason)]` when it tears one down
        for a lost lease -- two completely different bugs that look
        identical from the hub (the claim ref is simply absent). An
        assertion that cannot tell them apart sends the reader to guess,
        and the fixture's tempdir is deleted before they can look.
        """
        return (
            "\n--- fleetd log ---\n"
            + (driver.log() or "(empty)")
            + "\n--- stub gate runlog ---\n"
            + (gate._log() or "(empty)")
            + f"\n--- driver alive: {driver.alive()} ---"
        )

    # ---------------- seam 1 ------------------------------------------- #

    def assert_lease_holds_through_work(self, driver, slug: str, ttl: float, work_s: float):
        """SEAM 1. A gate that outlives its lease TTL holds a claim that is
        continuously live on the hub, unreapable by a third party, and
        released -- with its verdict written -- when the work ends.

        Every failure message here starts with `SEAM-1` so the negative
        control can assert it failed for the RIGHT reason rather than for
        any reason at all.
        """
        branch = f"staging/{slug}"
        ref = claim_ref("gate", branch.replace("/", "-"))
        observer = self.observer("liveness")
        third_party = self.observer("reaper")
        gate = driver.gate

        driver.start()
        self.wait_until(
            lambda: observer.sha(ref) is not None,
            timeout=60,
            what=f"SEAM-1 fleetd never claimed {ref} (driver={driver.kind}); daemon log:\n{driver.log()}",
        )
        self.wait_until(
            lambda: gate.starts() >= 1,
            timeout=60,
            what="SEAM-1 the stub gate was claimed but never started",
        )
        # Drain this host now. The gate already running is untouched
        # (fleetd drains, never kills) -- but nothing NEW starts, so the
        # release at the end of the hold is observable instead of being
        # instantly overwritten by the next gate on the same branch.
        self.set_desired(driver.host, gates=1, enabled=False)

        started = time.monotonic()
        poll = poll_interval(ttl)
        # Twice the hold plus a minute. Generous enough for a slow
        # start under load, bounded enough that a wedged stub gate
        # fails the run instead of holding a burn-in for an hour.
        deadline = started + work_s * 2 + 60
        expiries: list = []
        reap_probe_done = False
        last_live_at = 0.0

        while time.monotonic() < deadline:
            if gate.finished():
                break
            payload = observe_claim(observer, ref, f"seam-1 liveness read of {ref}")
            elapsed = time.monotonic() - started
            self.assertIsNotNone(
                payload,
                f"SEAM-1 LEASE LAPSED: {ref} vanished from the hub {elapsed:.1f}s into a "
                f"{work_s:g}s hold (ttl={ttl:g}s). A claim that disappears mid-work is "
                f"reapable by any host, which is two gates on one branch."
                + self.diagnosis(driver, gate)
                + self.ref_forensics(ref),
            )
            self.assertFalse(
                is_expired(payload),
                f"SEAM-1 LEASE LAPSED: {ref} is EXPIRED {elapsed:.1f}s into a {work_s:g}s hold "
                f"(ttl={ttl:g}s, expires_at={payload.get('expires_at')}). This is the original "
                f"defect verbatim: the work outlived the lease and nothing renewed it."
                + self.diagnosis(driver, gate),
            )
            last_live_at = elapsed
            expires_at = payload.get("expires_at")
            if not expiries or expiries[-1] != expires_at:
                expiries.append(expires_at)

            # The probe must land AFTER the nominal TTL (before it, a
            # never-renewed claim would still look live and the probe would
            # prove nothing) and with room to spare before the hold ends.
            # `ttl + poll` is the first observation that is certainly past
            # it; a multiplier like `ttl * 1.25` never fires at all in SLOW
            # mode, where 1.25*600 exceeds the whole run.
            if not reap_probe_done and elapsed > ttl + poll:
                # A third party runs the reaper mid-work, past the nominal
                # TTL. A renewed lease is invisible to it; an unrenewed one
                # is not.
                reaped = tolerating_the_hub_read_race(
                    lambda: reap_expired(third_party), "seam-1 third-party reap probe"
                )
                self.assertNotIn(
                    ref,
                    reaped,
                    f"SEAM-1 REAPED MID-WORK: a third party's reap_expired() collected {ref} "
                    f"{elapsed:.1f}s into a {work_s:g}s hold. The branch is now claimable by "
                    f"another host while this gate is still running.",
                )
                reap_probe_done = True
            time.sleep(poll)

        self.assertTrue(
            gate.finished(),
            f"SEAM-1 the stub gate never finished within {deadline - started:.0f}s; "
            f"runlog:\n{gate._log()}\ndaemon log:\n{driver.log()}",
        )
        self.assertTrue(
            reap_probe_done,
            "SEAM-1 the mid-work reap probe never ran -- the hold ended before the nominal "
            "TTL, so this test proved nothing about outliving a lease",
        )
        self.assertGreater(
            last_live_at,
            ttl,
            f"SEAM-1 the claim was only observed live for {last_live_at:.1f}s, which is inside "
            f"the {ttl:g}s TTL -- the property under test is what happens AFTER it",
        )
        self.assertGreaterEqual(
            len(expiries),
            3,
            f"SEAM-1 NO RENEWAL OBSERVED: expires_at took only {len(expiries)} distinct value(s) "
            f"({expiries}) across a {work_s:g}s hold at ttl={ttl:g}s. A lease that never moves "
            f"its deadline is not being renewed; it merely had not expired yet.",
        )

        # The end of the seam: work finished -> claim released -> memo written.
        self.wait_until(
            lambda: observer.sha(ref) is None,
            timeout=20 + 8 * FLEETD_INTERVAL_S,
            what=f"SEAM-1 CLAIM NOT RELEASED: {ref} still on the hub after the gate exited",
        )
        cached = verdict_mod.lookup(observer, gate.tree_sha, gate.gate_version, gate.platform_id)
        self.assertIsNotNone(
            cached,
            f"SEAM-1 VERDICT MEMO never fired: nothing at "
            f"{verdict_mod.verdict_ref(gate.tree_sha, gate.gate_version, gate.platform_id)} "
            f"after the gate completed. gate runlog:\n{gate._log()}",
        )
        self.assertEqual(cached["result"], "PASS")
        self.assertEqual(gate.starts(), 1, "SEAM-1 the stub gate ran more than once")
        report_hub_read_race_hits("SEAM-1")
        report_transient_absence_hits("SEAM-1")

    # ---------------- seam 2 ------------------------------------------- #

    def assert_exclusion_round_trip(self, driver, slug: str, ttl: float, work_s: float):
        """SEAM 2. Host A's fleetd holds the claim; host B computes its own
        queue from its own `Hub` and must not offer the branch -- for the
        WHOLE hold, including well past the nominal TTL -- and must offer
        it again once the claim is released.

        Failure messages start with `SEAM-2` for the negative control.
        """
        branch = f"staging/{slug}"
        ref = claim_ref("gate", branch.replace("/", "-"))
        hub_b = self.observer("host-b-queue")
        queue_b = workqueue.Queue(hub_b)
        gate = driver.gate

        driver.start()
        self.wait_until(
            lambda: self.observer("probe").sha(ref) is not None,
            timeout=60,
            what=f"SEAM-2 host A's fleetd never claimed {ref}; daemon log:\n{driver.log()}",
        )
        self.set_desired(driver.host, gates=1, enabled=False)

        started = time.monotonic()
        poll = queue_poll_interval(ttl)
        # Twice the hold plus a minute. Generous enough for a slow
        # start under load, bounded enough that a wedged stub gate
        # fails the run instead of holding a burn-in for an hour.
        deadline = started + work_s * 2 + 60
        checks_past_ttl = 0

        while time.monotonic() < deadline:
            if gate.finished():
                break
            with self.as_host(self.HOST_B):
                queued = tolerating_the_hub_read_race(
                    queue_b.compute, "seam-2 host-B queue computation"
                )
            elapsed = time.monotonic() - started
            self.assertNotIn(
                slug,
                queued,
                f"SEAM-2 EXCLUSION LOST: host B's queue offered {slug!r} {elapsed:.1f}s into a "
                f"{work_s:g}s gate held by host A (ttl={ttl:g}s). Host A's claim payload "
                f"work_key is {branch!r}; host B's queue key is the slug {slug!r} / the ref "
                f"{'refs/heads/' + branch!r}. Two hosts now gate the same branch.",
            )
            if elapsed > ttl:
                checks_past_ttl += 1
            time.sleep(poll)

        self.assertTrue(
            gate.finished(),
            f"SEAM-2 the stub gate never finished; runlog:\n{gate._log()}",
        )
        self.assertGreaterEqual(
            checks_past_ttl,
            2,
            f"SEAM-2 only {checks_past_ttl} exclusion check(s) happened after the {ttl:g}s TTL "
            f"elapsed -- renewal-keeps-it-excluded is exactly the half this test exists for",
        )

        # ...and it comes back. An exclusion that never lifts is a leak,
        # not a lease.
        def returned():
            with self.as_host(self.HOST_B):
                queued = tolerating_the_hub_read_race(
                    queue_b.compute, "seam-2 host-B requeue check"
                )
            return slug in queued

        self.wait_until(
            returned,
            timeout=30 + 8 * FLEETD_INTERVAL_S,
            what=f"SEAM-2 NOT REQUEUED: {slug!r} never returned to host B's queue after host A "
            f"released {ref}",
        )
        report_hub_read_race_hits("SEAM-2")


# --------------------------------------------------------------------- #
# SEAM 1 -- lease through work
# --------------------------------------------------------------------- #


class TestSeam1LeaseThroughWork(FleetdSeamFixture):
    """R2's property, composed: fleetd's real loop, a real subprocess gate
    that outlives the TTL, a second Hub watching, a third reaping."""

    def test_claim_is_live_throughout_a_gate_that_outlives_its_ttl(self):
        ttl, renew, work = timescale()
        self.add_staging_branch("s1", {"s1.txt": "s1\n"})
        self.set_desired(self.HOST_A, gates=1, enabled=True)
        gate = self.make_gate("s1gate", "staging/s1", work)
        driver = SubprocessFleetd(self, self.HOST_A, gate, ttl, renew)
        self.addCleanup(driver.stop)
        self.assert_lease_holds_through_work(driver, "s1", ttl, work)


# --------------------------------------------------------------------- #
# SEAM 2 -- exclusion round-trip across hosts
# --------------------------------------------------------------------- #


class TestSeam2ExclusionRoundTrip(FleetdSeamFixture):
    """R4's round-trip: 'a fleetd-held claim excludes its branch from
    another host's queue computation'."""

    def setUp(self):
        if not queue_key_fix_present():
            self.skipTest(QUEUE_KEY_SKIP_REASON)
        super().setUp()

    def test_branch_stays_out_of_another_hosts_queue_for_the_whole_gate(self):
        ttl, renew, work = timescale()
        self.add_staging_branch("s2", {"s2.txt": "s2\n"})
        self.set_desired(self.HOST_A, gates=1, enabled=True)
        gate = self.make_gate("s2gate", "staging/s2", work)
        driver = SubprocessFleetd(self, self.HOST_A, gate, ttl, renew)
        self.addCleanup(driver.stop)
        self.assert_exclusion_round_trip(driver, "s2", ttl, work)


# --------------------------------------------------------------------- #
# SEAM 3 -- the train, end to end
# --------------------------------------------------------------------- #


class TestSeam3TrainEndToEnd(SeamFixture):
    """The real `run_train` against a real fixture hub, with a stub gate
    driven by a CONTROL FILE -- poison is data the fixture writes, not a
    branch in the test code, so the same gate function serves every case.
    """

    def setUp(self):
        super().setUp()
        self.push_log = self.push_log_hook()
        self.control = self.tmp / "poison.control"
        self.control.write_text("")
        self.gate_calls: list = []
        self.concurrent: dict = {}

    def poison(self, *slugs):
        self.control.write_text("\n".join(slugs) + "\n")

    def gate_fn(self, clone, label):
        """PASS unless the gated member set intersects the control file.

        The set is parsed from the label, never substring-matched: a slug
        that is a prefix of another must not be able to poison it.
        """
        self.gate_calls.append(label)
        members = {p for p in label.split("+") if p and p != "retry"}
        poisoned = {l.strip() for l in self.control.read_text().splitlines() if l.strip()}
        return "FAIL" if members & poisoned else "PASS"

    def contending_gate_fn(self, clone, label):
        """Same gate, but the FIRST invocation starts a second train while
        this one holds the singleton. A train that is gating is a train
        holding a claim for 20-45 minutes; a second cron tick landing in
        that window is the normal case, not the exotic one."""
        if "second" not in self.concurrent:
            self.concurrent["second"] = train.run_train(
                self.hub_path,
                self.tmp,
                gate_fn=lambda c, l: self.fail("the second train must never gate anything"),
                epoch="seam3-B",
                hub_workdir=self.tmp / "traincache-b",
            )
        return self.gate_fn(clone, label)

    def run_the_train(self, gate_fn=None, epoch="seam3-A"):
        return train.run_train(
            self.hub_path,
            self.tmp,
            gate_fn=gate_fn or self.gate_fn,
            epoch=epoch,
            hub_workdir=self.tmp / "traincache-a",
        )

    def test_poison_ejected_survivors_land_and_the_tip_advances_exactly_once(self):
        shas = {slug: self.add_staging_branch(slug, {f"{slug}.txt": f"{slug}\n"})
                for slug in ("ta", "tb", "tpoison")}
        self.poison("tpoison")
        before = self.tip_sha()

        res = self.run_the_train(gate_fn=self.contending_gate_fn)

        self.assertEqual(res.outcome, "advanced", f"gate calls: {self.gate_calls}")
        self.assertEqual(sorted(res.landed), ["staging/ta", "staging/tb"])
        self.assertIn(("staging/tpoison", "gate FAIL"), res.ejected)

        # The tip advanced EXACTLY once -- counted at the hub, by the hub.
        tip_pushes = self.pushes_to(self.push_log, TIP_REF)
        self.assertEqual(
            len(tip_pushes),
            1,
            f"the tip must be written exactly once per run; hub saw {tip_pushes}",
        )
        self.assertEqual(tip_pushes[0][0], before, "the one push must build on the tip we read")
        self.assertEqual(self.tip_sha(), res.new_tip)
        self.assertNotEqual(self.tip_sha(), before)

        # Staging refs retired only for what landed; the poison keeps its
        # branch (an ejection deletes nothing).
        self.assertEqual(
            sorted(self.hub.list("refs/heads/staging/")), ["refs/heads/staging/tpoison"]
        )
        # ...and every retirement was preceded by a VERIFIED rescue.
        rescued = self.hub.list("refs/heads/rescued/")
        self.assertEqual(
            rescued,
            {"refs/heads/rescued/ta": shas["ta"], "refs/heads/rescued/tb": shas["tb"]},
            "a staging ref may only be retired once its exact gated commit is "
            "verifiably reachable under rescued/",
        )
        self.assertEqual(res.retire_failures, [])

        # The second train, started from inside the first one's gate.
        self.assertIn("second", self.concurrent, "the concurrent run never happened")
        self.assertEqual(
            self.concurrent["second"].outcome,
            "claim-held",
            "a second train while the first holds refs/fleet/claims/train/singleton must "
            "refuse -- keyed by epoch (the pre-fix shape) both would proceed",
        )
        # Released on the way out, so the next cron tick is not blocked.
        self.assertEqual(self.hub.list("refs/fleet/claims/"), {})

    def test_union_that_failed_can_never_be_pushed_and_the_tip_does_not_move(self):
        """The structural guard at the push site, in composition.

        Forge the exact memo state the pre-fix bisect produced -- the
        survivor set marked FAILED, nothing marked PASSED, members handed
        back anyway -- and the train must refuse. Before R3 this pushed the
        tree whose gate had just said FAIL.
        """
        for slug in ("ua", "ub"):
            self.add_staging_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        before = self.tip_sha()
        real_bisect = train._gate_and_bisect

        def union_failed(clone, tip_sha, members, gate_fn, res, memo=None):
            real_bisect(clone, tip_sha, members, gate_fn, res, memo)
            if memo is not None:
                memo.passed.clear()
                memo.failed.add(train._memo_key(members))
            return members

        with mock.patch.object(train, "_gate_and_bisect", union_failed):
            with self.assertRaises(train.TrainError) as ctx:
                self.run_the_train()

        self.assertIn("never gated as exactly that set", str(ctx.exception))
        self.assertEqual(self.tip_sha(), before, "the tip must not move")
        self.assertEqual(
            self.pushes_to(self.push_log, TIP_REF), [], "the hub must have seen no tip write at all"
        )
        self.assertEqual(self.hub.list("refs/heads/rescued/"), {}, "nothing may be rescued either")
        self.assertEqual(len(self.hub.list("refs/heads/staging/")), 2, "and nothing retired")

    def test_landed_branch_flips_its_intent_to_done(self):
        """A branch that lands on the tip completes the intent it was
        registered against.

        `train.py`'s retire path (`_retire_staging_ref` -> `_mark_intent_done`)
        CAS-updates `refs/fleet/intents/<slug>` from "open" to "done" once
        the branch's staging ref is verifiably retired, recording
        `landed_sha` + `landed_at` via `intent.mark_done` (ARCH-FIX FIX-3a).
        Before that write existed, `intent.py` only ever wrote "open"
        (`register`) and "withdrawn" (`withdraw`), so a completed intent
        stayed "open" forever and `fleetd`'s authoring path kept offering it
        as work to author for as long as the ref existed -- it was skipped
        only while `refs/heads/staging/<slug>` existed, and the train had
        just deleted that branch.
        """
        self.add_staging_branch("ti", {"ti.txt": "ti\n"})
        intent_ref = "refs/fleet/intents/ti"
        self.assertTrue(
            self.hub.create(
                intent_ref,
                {"slug": "ti", "title": "seam intent", "scope": {}, "status": "open",
                 "claimed_by": "seam", "created_at": "2026-08-15T00:00:00+00:00"},
            )
        )
        res = self.run_the_train()
        self.assertEqual(res.landed, ["staging/ti"])
        payload = self.hub.read(intent_ref)
        self.assertIsNotNone(payload, "the intent ref itself vanished")
        self.assertEqual(
            payload.get("status"),
            "done",
            "an intent whose branch has landed on the tip is still 'open' -- nothing in "
            "tools/fleet ever writes 'done'",
        )


# --------------------------------------------------------------------- #
# SEAM 4 -- restart adoption
# --------------------------------------------------------------------- #


@unittest.skipUnless(ADOPTION_SUPPORTED, ADOPTION_SKIP_REASON)
class TestSeam4RestartAdoption(FleetdSeamFixture):
    """R6 + R8, composed: 'kill a fixture fleetd mid-gate, start a new one,
    observe adoption or clean reap -- no lost slot, no double gate.'

    The successor runs under the REAL supervisor
    (`tools/fleet/units/fleetd-wrapper.sh`, R8), not as a single hand-run
    process, because that is what decides whether a restart succeeds at
    all: `fleetd.main` exits 3 when the host singleton is still held, and
    only the wrapper's retry loop ever gets past that. A test that starts
    one successor by hand measures a daemon that gave up half a second
    after the kill, and calls the result "adoption failed".

    Split into two tests on purpose:

      * `test_successor_adopts_...` -- what R6 actually delivers, and it
        does: the ref is never absent (adoption is a CAS `update`, not a
        delete-and-recreate), the gate is never started twice, the claim
        is released when the work ends.
      * `test_claim_never_expires_...` -- the stronger property this
        suite's brief asked for, RED, pinning a real composition defect.
        See its docstring.
    """

    def run_restart_scenario(self):
        """SIGKILL a supervised fleetd mid-gate; watch the claim across the
        handover. Returns (gate, ref, observations, driver)."""
        ttl, renew, base_work = timescale()
        # The hold has to outlast the whole handover -- kill, singleton
        # lockout, wrapper retry, adoption -- so it is longer here than in
        # seam 1.
        work = base_work * 3
        self.add_staging_branch("s4", {"s4.txt": "s4\n"})
        self.set_desired(self.HOST_A, gates=1, enabled=True)
        gate = self.make_gate("s4gate", "staging/s4", work)
        ref = claim_ref("gate", "staging-s4")
        observer = self.observer("s4-liveness")

        driver = SupervisedFleetd(self, self.HOST_A, gate, ttl, renew)
        self.addCleanup(driver.stop)
        driver.start()
        self.wait_until(lambda: gate.starts() >= 1, timeout=90,
                        what=f"SEAM-4 the first fleetd never started the gate:\n{driver.log()}")
        self.wait_until(lambda: observer.sha(ref) is not None, timeout=90,
                        what=f"SEAM-4 the first fleetd never claimed {ref}:\n{driver.log()}")
        victim = self.wait_for_fleetd_pid(driver)

        # Kill the DAEMON, not its process group: gates are spawned with
        # `start_new_session=True`, so a group kill would take the gate
        # with it and the restart seam would be testing nothing. The
        # wrapper survives (it is the parent) and restarts fleetd, exactly
        # as it would on the pod.
        killed_at = time.monotonic()
        os.kill(victim, signal.SIGKILL)

        # Drain the host so that, once a successor is finally in, it
        # adopts the running gate rather than also being free to start a
        # new one -- the property under test is the handover, not the
        # scheduler.
        self.set_desired(self.HOST_A, gates=1, enabled=False)

        observations: list = []
        poll = poll_interval(ttl)
        deadline = killed_at + work * 2 + 120
        while time.monotonic() < deadline:
            if gate.finished():
                break
            payload = observe_claim(observer, ref, f"seam-4 handover read of {ref}")
            elapsed = time.monotonic() - killed_at
            # BLOCKER 7 (seam-4 unmasking): `started_at` is carried alongside
            # `state` on every observation, not just derived from it.
            # `(holder_host, started_at)` IS the claim's ownership token
            # (claim.py's `_owns`, see its module docstring) -- a successor
            # that ADOPTS continues the token; one that REACQUIRES (delete +
            # create) mints a fresh one. Never observing "absent" only shows
            # the poll never SAMPLED a gap; it does not show there was no
            # gap. A changed `started_at` between two "live"/"expired"
            # observations is adopt-vs-reacquire made positive evidence
            # instead of inferred from the absence of an observed absence.
            if payload is None:
                state = "absent"
                started_at = None
            elif is_expired(payload):
                state = "expired"
                started_at = payload.get("started_at")
            else:
                state = "live"
                started_at = payload.get("started_at")
            observations.append((round(elapsed, 2), state, started_at))
            time.sleep(poll)

        self.assertTrue(
            gate.finished(),
            f"SEAM-4 the gate never finished; runlog:\n{gate._log()}\n{driver.log()}",
        )
        return gate, ref, observations, driver, observer

    def wait_for_fleetd_pid(self, driver, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            pid = driver.fleetd_pid()
            if pid is not None:
                return pid
            time.sleep(0.2)
        self.fail(f"SEAM-4 no fleetd child under the wrapper to kill:\n{driver.log()}")

    def test_successor_adopts_the_running_gate_with_no_double_gate(self):
        gate, ref, observations, driver, observer = self.run_restart_scenario()

        self.assertEqual(
            gate.starts(), 1,
            f"SEAM-4 DOUBLE GATE: the stub gate was started {gate.starts()} times across the "
            f"restart. One branch, two gates, is the exact event leases exist to prevent.\n"
            f"{driver.log()}",
        )
        absent = [o for o in observations if o[1] == "absent"]
        self.assertEqual(
            absent, [],
            f"SEAM-4 CLAIM WENT ABSENT: {ref} disappeared from the hub during the handover at "
            f"{absent[:8]}. R6's adoption is specified as a CAS `update` from the sha already "
            f"on the hub precisely so the ref's EXISTENCE is continuous; a gap here means the "
            f"successor re-acquired (delete + create) instead of adopting, and in that gap the "
            f"branch reads as unclaimed to every other host.\n{driver.log()}",
        )
        self.assertTrue(
            any(state == "live" for _t, state, _started_at in observations),
            f"SEAM-4 the claim was never observed live after the kill -- no successor ever "
            f"adopted it:\n{driver.log()}",
        )
        # BLOCKER 7 (seam-4 unmasking): the ownership token's `started_at`
        # half must be the SAME value on every non-absent observation.
        # "never observed absent" (above) only proves the poll never
        # SAMPLED a gap -- it is silent on whether the successor adopted
        # the running claim or reacquired it (delete then recreate) inside
        # a gap the poll happened to miss. A changed `started_at` between
        # two observations is exactly that reacquisition, made positive
        # evidence: `claim.py`'s `(holder_host, started_at)` token only
        # stays constant across a real `Claim.adopt` (continues the lease)
        # and changes on any fresh `acquire`/`create`.
        tokens = [(t, sa) for t, _state, sa in observations if sa is not None]
        self.assertTrue(
            tokens, f"SEAM-4 no non-absent observation carried a started_at at all -- "
            f"the token itself was never captured:\n{driver.log()}",
        )
        distinct = sorted(set(sa for _t, sa in tokens))
        self.assertEqual(
            len(distinct), 1,
            f"SEAM-4 OWNERSHIP TOKEN CHANGED: started_at took on {len(distinct)} distinct "
            f"values across the handover ({distinct}) -- the successor REACQUIRED the claim "
            f"(delete + create, a fresh started_at) instead of ADOPTING it (a CAS update that "
            f"preserves started_at), even though the ref was never observed absent. Full "
            f"trace: {[(t, sa) for t, sa in tokens]}\n{driver.log()}",
        )
        self.wait_until(
            lambda: observer.sha(ref) is None,
            timeout=60 + 8 * FLEETD_INTERVAL_S,
            what=f"SEAM-4 the adopted claim {ref} was never released when the work finished:\n"
                 f"{driver.log()}",
        )

    def test_claim_never_expires_across_the_handover(self):
        """WAS RED ON PURPOSE -- a composition defect, not a bent test.
        Fixed by ARCH-FIX FIX 2; kept green here as the seam's acceptance.

        A hard-killed fleetd leaves its OWN host-singleton claim
        (`refs/fleet/claims/host/<host>`) sitting on the hub, unrenewed but
        NOT YET EXPIRED. `fleetd.main` used to take that claim with only
        `acquire_or_reap`, which reaps only an EXPIRED claim, so every
        successor exited 3 ("another instance holds
        refs/fleet/claims/host/<host>") until the dead daemon's lease ran
        out -- one full `LEASE_TTL`: 600 seconds in production, 5 in CI
        mode. Previously measured here: three refused restarts, then
        adoption, with the claim observed EXPIRED for that whole window.

        Two consequences, and the second is the one this test pins.

        1. The host runs NO scheduler for a whole TTL after any crash. Not
           a lease bug, but ten production minutes of a host that starts
           nothing, finishes nothing, and writes no heartbeat -- while
           `fleet status` shows its last heartbeat going stale and every
           gate it was supervising runs unwatched.
        2. The gate claims it was renewing expire at almost exactly the
           same instant its singleton does, because the same dead daemon
           renewed all of them from the same moment. So the successor
           becomes able to adopt at precisely the moment the gate claim
           becomes reapable, and whether the claim survives is a race
           between this host's restart and any other host's
           `reap_expired`. Losing it means another host claims a branch
           this host is still gating -- two gates on one branch, the exact
           event leases exist to prevent.

        R6's adoption code was already correct and did the right thing the
        moment it could run: `Claim.adopt` continues the lease by CAS
        update, and `test_successor_adopts_the_running_gate_with_no_double_
        gate` above passes, adoption line and all. It simply could not run
        until the lockout ended, and by then the lease it was adopting had
        expired.

        FIX 2 closes the lockout itself, not adoption: `fleetd.main`'s
        singleton block now falls back to `reap_dead_same_host_singleton`
        when `acquire_or_reap` refuses -- the same evidence `adopt_workers`
        already used for gate/agent claims (`claim.holder_host == us AND
        claim.pgid is provably dead by the `ps` listing`), applied to the
        daemon's own claim instead of the work it supervises. See that
        function's docstring for the one complication unique to this
        level: fleetd shares its supervisor's process group under R8's
        wrapper (no `setsid`), so identity is checked against every member
        of the group, not just its leader, and the successor's own pid is
        excluded -- otherwise a fresh fleetd matches ITSELF and concludes
        the dead predecessor is alive forever.
        """
        gate, ref, observations, driver, _observer = self.run_restart_scenario()
        expired = [o for o in observations if o[1] == "expired"]
        window = (expired[-1][0] - expired[0][0]) if len(expired) > 1 else 0.0
        self.assertEqual(
            expired, [],
            f"SEAM-4 CLAIM EXPIRED ACROSS THE HANDOVER: {ref} was expired for ~{window:.1f}s "
            f"({len(expired)} observations, first at +{expired[0][0] if expired else 0}s after "
            f"the SIGKILL) while its gate kept running. Any host's reaper could have collected "
            f"it and started the same branch.\nobservations: {observations}\n{driver.log()}",
        )


# --------------------------------------------------------------------- #
# SEAM 5 -- hook enforcement, in composition
# --------------------------------------------------------------------- #


class TestSeam5HookEnforcementInComposition(SeamFixture):
    """R1 in the path that actually writes the tip.

    `test_update_hook.py` proves the hook denies a hand-made push. This
    proves the composition: the REAL installer, the REAL train, the REAL
    token file, a rogue clone that has everything except the token, and
    the post-receive chain still intact behind the new pre-receive.
    """

    def install_hooks(self):
        result = subprocess.run(
            ["bash", str(INSTALL_HOOK), self.hub_path, "--execute"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        token_file = Path(self.hub_path) / "train.token"
        self.assertTrue(token_file.is_file(), "installer did not create train.token")
        self.assertEqual(token_file.stat().st_mode & 0o777, 0o600)
        return token_file

    def generation(self):
        payload = self.hub.read(fleetd.TIP_SIGNAL_REF)
        return None if payload is None else payload.get("generation")

    def test_train_push_lands_rogue_push_denied_and_the_signal_chain_survives(self):
        sha = self.add_staging_branch("h1", {"h1.txt": "h1\n"})
        token_file = self.install_hooks()
        # The train reads the token from this path (R1's hub-local file).
        self.set_env(**{train.TRAIN_TOKEN_ENV: str(token_file)})
        before_tip = self.tip_sha()
        before_gen = self.generation()

        res = train.run_train(
            self.hub_path,
            self.tmp,
            gate_fn=lambda clone, label: "PASS",
            epoch="seam5",
            hub_workdir=self.tmp / "traincache",
        )

        # 1. The train's token'd push to the protected ref LANDS.
        self.assertEqual(res.outcome, "advanced", "the train's tokened tip push was refused")
        self.assertEqual(res.landed, ["staging/h1"])
        self.assertNotEqual(self.tip_sha(), before_tip)
        self.assertEqual(self.hub.list("refs/heads/rescued/"), {"refs/heads/rescued/h1": sha})
        after_train_tip = self.tip_sha()

        # 2. post-receive still fires BEHIND the new pre-receive: the
        #    generation signal advanced on the train's allowed push. The
        #    chain is what the installer promises; this is the end-to-end
        #    proof that adding a denying hook did not silence it.
        after_gen = self.generation()
        self.assertIsNotNone(
            after_gen,
            "no refs/fleet/signals/tip after the train's push -- the post-receive chain is "
            "broken behind the new pre-receive guard",
        )
        self.assertGreater(
            after_gen,
            before_gen or 0,
            f"tip signal generation did not advance ({before_gen} -> {after_gen})",
        )

        # 3. A rogue agent with a clone and no token is DENIED on the tip.
        rogue = self.tmp / "rogue"
        git(["clone", "--quiet", self.hub_path, str(rogue)])
        git(["config", "user.email", "rogue@t"], cwd=rogue)
        git(["config", "user.name", "rogue"], cwd=rogue)
        git(["checkout", "--quiet", "-B", "rogue-work", after_train_tip], cwd=rogue)
        (rogue / "rogue.txt").write_text("straight to the tip\n")
        git(["add", "."], cwd=rogue)
        git(["commit", "--quiet", "-m", "rogue"], cwd=rogue)
        denied = git(["push", "origin", f"HEAD:{TIP_REF}"], cwd=rogue, check=False)
        self.assertNotEqual(denied.returncode, 0, "a tokenless push to the tip was ACCEPTED")
        self.assertIn("tip-guard: DENY", denied.stderr)
        self.assertEqual(self.tip_sha(), after_train_tip, "the denied push still moved the tip")

        # 4. staging/* is unaffected -- the guard protects two refs, not the
        #    fleet's ability to work.
        allowed = git(["push", "origin", "HEAD:refs/heads/staging/rogue"], cwd=rogue, check=False)
        self.assertEqual(allowed.returncode, 0, msg=allowed.stderr)
        self.assertIsNotNone(self.hub.sha("refs/heads/staging/rogue"))
        # ...and a staging push does not bump the tip signal.
        self.assertEqual(self.generation(), after_gen)


# --------------------------------------------------------------------- #
# SEAM 6 -- the negative control for the whole suite
# --------------------------------------------------------------------- #


@contextlib.contextmanager
def renewer_start_in_acquire_disabled():
    """Disable EXACTLY ONE LINE of R2's fix: the `self.start_renewer()`
    call at the end of `Claim.acquire()`.

    That line is the whole of the lease fix. Before it existed, `fleetd`
    took every gate, agent and host-singleton claim through
    `acquire_or_reap()` and never renewed any of them, so every claim held
    longer than the TTL -- i.e. every real gate -- expired mid-work while
    three unit-test suites stayed green.

    The patch shadows `start_renewer` with a no-op on the instance for the
    duration of `acquire` only, then removes the shadow. Nothing else
    changes: `release()`, `stop_renewer()` and an explicit
    `start_renewer()` from a caller all still work, so what is being
    measured is that one call site and not the renewer machinery.
    """
    real_acquire = claim_mod.Claim.acquire

    def acquire_without_starting_the_renewer(self):
        self.start_renewer = lambda: None  # instance attr shadows the method
        try:
            return real_acquire(self)
        finally:
            try:
                del self.start_renewer
            except AttributeError:
                pass

    with mock.patch.object(claim_mod.Claim, "acquire", acquire_without_starting_the_renewer):
        yield


class TestSeam6NegativeControl(FleetdSeamFixture):
    """Would this suite have caught the original defect?

    A test suite that cannot be made to fail is not evidence. These tests
    disable the one production line the lease fix consists of and require
    seams 1 and 2 to go RED -- with the right message -- then restore it
    and require them GREEN again.

    They use the in-process driver (`fleetd.main`'s loop body in a thread)
    because a `mock.patch` in this interpreter cannot reach the subprocess
    daemon the other seams use. The assertions are the very same methods,
    called on the very same fixture.
    """

    def setUp(self):
        super().setUp()
        ttl, renew, _work = timescale()
        # In-process claims read these at construction (claim.py's
        # documented compression knobs).
        self.set_env(FLEET_TEST_TTL_S=ttl, FLEET_TEST_RENEW_S=renew)

    def _run_seam1(self, slug: str, ttl: float, work: float):
        self.clear_staging()
        self.add_staging_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        self.set_desired(self.HOST_A, gates=1, enabled=True)
        gate = self.make_gate(f"{slug}gate", f"staging/{slug}", work)
        driver = InProcessFleetd(self, self.HOST_A, gate)
        try:
            self.assert_lease_holds_through_work(driver, slug, ttl, work)
        finally:
            gate.stop()
            driver.stop()

    def _run_seam2(self, slug: str, ttl: float, work: float):
        self.clear_staging()
        self.add_staging_branch(slug, {f"{slug}.txt": f"{slug}\n"})
        self.set_desired(self.HOST_A, gates=1, enabled=True)
        gate = self.make_gate(f"{slug}gate", f"staging/{slug}", work)
        driver = InProcessFleetd(self, self.HOST_A, gate)
        try:
            self.assert_exclusion_round_trip(driver, slug, ttl, work)
        finally:
            gate.stop()
            driver.stop()

    def test_seam1_goes_red_without_the_renewer_start_and_green_with_it(self):
        ttl, _renew, work = timescale()
        with renewer_start_in_acquire_disabled():
            with self.assertRaises(AssertionError) as ctx:
                self._run_seam1("nc1", ttl, work)
        message = str(ctx.exception)
        self.assertIn(
            "SEAM-1",
            message,
            f"seam 1 failed, but not for a lease reason -- the negative control proves nothing "
            f"unless the failure is the lease. Got: {message}",
        )
        self.assertTrue(
            "LEASE LAPSED" in message or "REAPED MID-WORK" in message,
            f"expected the lease-liveness assertion to be the one that broke; got: {message}",
        )
        # Restored: the same composition, the same fixture, now green.
        self._run_seam1("nc2", ttl, work)

    def test_seam2_goes_red_without_the_renewer_start_and_green_with_it(self):
        if not queue_key_fix_present():
            self.skipTest(
                "SEAM 2's negative control is meaningless while R4's key mismatch is unfixed: "
                "the exclusion never holds at all, so disabling renewal cannot be what breaks "
                "it. Re-run once T5 (`staging/afx-queue`) lands. " + QUEUE_KEY_SKIP_REASON
            )
        ttl, _renew, work = timescale()
        with renewer_start_in_acquire_disabled():
            with self.assertRaises(AssertionError) as ctx:
                self._run_seam2("nc3", ttl, work)
        self.assertIn("SEAM-2", str(ctx.exception))
        self._run_seam2("nc4", ttl, work)


# --------------------------------------------------------------------- #
# SEAM 7 -- found BY this suite: reading a claim that renews underneath you
# --------------------------------------------------------------------- #


class TestSeam7HubReadRaceUnderRenewal(SeamFixture):
    """WAS RED ON PURPOSE; NOW GREEN -- a production defect this suite
    found, reproduced deterministically, and now pins fixed.

    History, kept because the `expectedFailure` that used to sit on the
    test below is gone and this is the only remaining record of why it was
    there. `fleetlib.Hub.read` used to resolve the ref's sha, THEN fetch the
    ref, THEN cat-file the sha it resolved first:

        found_sha = self._remote_sha(ref)              # ls-remote -> S1
        self._run(["fetch", ..., f"+{ref}:{tmp_ref}"]) # brings whatever
                                                       # the ref is NOW
        self._run(["cat-file", "-p", f"{found_sha}:payload.json"])

    A ref that moves in that window leaves the fetch bringing S2 while the
    cat-file asks for S1, which was never fetched -- and payload commits
    are orphans, so nothing else drags S1 into the object store. Git's
    message for an object it does not have is
    `fatal: path 'payload.json' does not exist in '<sha>'` (verified
    directly, on git 2.54: it is NOT "invalid object name"), and
    `Hub.read` raises `HubError` for it.

    WHY IT MATTERS NOW. Before R2 nothing renewed a claim, so claim refs
    never moved and this was unreachable. R2 makes every held claim
    rewrite its ref once per renewal interval; R4 makes
    `workqueue.Queue.compute()` read EVERY claim payload on every call.
    `fleetd.reconcile_once` calls `compute()` on every loop with no
    `try/except` around it, and `fleetd.main` has none either -- so a
    `HubError` raised here does not degrade a queue, it exits the daemon.
    `claim.reap_expired`, `claim.is_workdir_claimed` and `Claim.renew`'s
    own re-read take the same path.

    HOW IT WAS FOUND. Not by inspection: an unmodified seam-2 run raised

        fleetlib.HubError: refs/fleet/claims/gate/staging-nc4@abbfaa31...
        has no readable payload.json: ... fatal: path 'payload.json' does
        not exist in 'abbfaa31...'

    while host B computed its queue and host A's fleetd renewed the claim
    it was reading. Compressed TTLs make the window frequent; they do not
    invent it.

    The interleaving is forced here rather than raced, so this test is
    deterministic -- which is what let it carry an `expectedFailure` without
    flaking, and what makes it a real regression test now that it does not.

    THE FIX (`fleetlib.Hub.read_with_sha`, which `read` now delegates to):
    fetch into a uuid-named local `tmp_ref`, then `rev-parse` and `cat-file`
    against THAT ref. The ls-remote stays, but only to tell ABSENT from
    UNREACHABLE -- the sha it resolves is discarded, because it is a fact
    about the past by the time the fetch runs. A local ref no remote can
    move is the property that closes the window; `read_with_sha` exists so
    callers that need a sha AND its payload get them from the same fetched
    commit instead of two independent observations.

    So this test asserts, in order: the forced renewal really landed (the
    ref really did move inside the window -- otherwise the test proves
    nothing), and the read still returned the payload rather than raising.
    """

    def test_reading_a_claim_that_renews_between_ls_remote_and_fetch(self):
        holder_hub = self.observer("race-holder")
        reader_hub = self.observer("race-reader")  # never read this ref before
        held = Claim(
            holder_hub,
            kind="gate",
            key="raceprobe",
            work_kind="gate",
            work_key="staging/raceprobe",
            ttl=600,
            renew_interval=300,
        )
        held.acquire()
        held.stop_renewer()  # renewal is driven by hand, so the window is exact
        self.addCleanup(held.release)

        real_remote_sha = reader_hub._remote_sha
        renewed = []

        def sha_then_move(ref):
            sha = real_remote_sha(ref)
            if ref == held.ref and not renewed:
                # The holder's 120s renewal lands in the reader's window.
                # In production this is a coincidence; here it is arranged,
                # because a test that reproduces a race one run in twenty
                # is not a regression test.
                renewed.append(held.renew())
            return sha

        with mock.patch.object(reader_hub, "_remote_sha", sha_then_move):
            payload = reader_hub.read(held.ref)

        self.assertTrue(renewed and renewed[0], "the forced renewal did not land")
        self.assertIsNotNone(payload, "Hub.read returned None for a ref that exists")
        self.assertEqual(payload.get("work_key"), "staging/raceprobe")


# --------------------------------------------------------------------- #
# SLOW mode -- the same seam at production timescale (stage-4 burn-in)
# --------------------------------------------------------------------- #


@unittest.skipUnless(
    SLOW,
    "slow burn-in: set FLEET_SEAMS_SLOW=1 to hold one lease for thirteen real minutes at the "
    "production 600s TTL / 120s renewal. Run in stage 4, not in CI.",
)
class TestSlowBurnInRealTimescale(FleetdSeamFixture):
    """The compression in CI mode is a claim about production, and this is
    the test that cashes it: the SAME assertions, with `FLEET_TEST_TTL_S`
    and `FLEET_TEST_RENEW_S` left at production values and a hold of
    thirteen minutes -- past the ten-minute mark at which every gate this
    fleet ever ran silently lost its lease.
    """

    def test_lease_holds_for_thirteen_real_minutes(self):
        ttl, renew, work = SLOW_TTL_S, SLOW_RENEW_S, SLOW_WORK_S
        self.assertGreater(work, ttl + renew, "the hold must outlive a real TTL, not approach it")
        self.add_staging_branch("slow", {"slow.txt": "slow\n"})
        self.set_desired(self.HOST_A, gates=1, enabled=True)
        gate = self.make_gate("slowgate", "staging/slow", work)
        driver = SubprocessFleetd(self, self.HOST_A, gate, ttl, renew)
        self.addCleanup(driver.stop)
        self.assert_lease_holds_through_work(driver, "slow", ttl, work)


if __name__ == "__main__":
    unittest.main()
