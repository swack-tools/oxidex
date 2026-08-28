#!/usr/bin/env python3
"""Tests for tools/fleet/intent.py -- the intent registry.

Fixture discipline for the HUB (the `Hub` instance backing
`refs/fleet/intents/*`) matches `test_fleetlib.py`/`test_claim.py`: every
test spins up its own throwaway `git init --bare` repo under the system
temp directory in `setUp`, asserted before any test body runs, and NEVER
touches the production hub (work2.oxidex.net).

The SOURCE repo (used for `git log` history checks and for building/running
the oxidex binary the capability ledger measures) is a separate concern --
it is THIS checkout (`REPO_ROOT` below), the same real repo `test_ledger.py`
uses, because the whole point of the acceptance test below is to prove a
real, un-mocked capability ledger refuses a real duplicate at the real tip.

THE NON-NEGOTIABLE ACCEPTANCE TEST is
`TestAcceptance.test_solo_ryzen5_intent_is_refused_by_capability_ledger`.
Per the T1.4 task brief: if it registers successfully, the capability-ledger
check is decorative, and the fix belongs in `ledger.py`/`intent.py`, not in
loosening this test.

Plain `unittest`, standard library only (no pytest in this environment).

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import ledger  # noqa: E402
from fleetlib import Hub  # noqa: E402
from intent import (  # noqa: E402
    check_capability_ledger,
    check_history,
    check_open_intent_overlap,
    intent_ref,
    list_open_intents,
    register,
    withdraw,
)
from _env import HermeticCase  # noqa: E402
from _fixtures import hub_spec, make_hub  # noqa: E402
from _mp import pool_context  # noqa: E402

# This module's own pools name their start method here, at their own call
# sites, and NOTHING here touches the process-global default -- see
# `tests/_mp.py` for why that distinction is the whole point.
_MP_CONTEXT = pool_context()

REPO_ROOT = Path(__file__).resolve().parents[3]


def _run_git(args, cwd=None, input_bytes=None):
    return subprocess.run(args, cwd=cwd, input=input_bytes, capture_output=True)


def _oracle_available() -> bool:
    return ledger.probe_capability().ok


def _corpus_available() -> bool:
    return ledger.CORPUS_DIR.is_dir()


# --------------------------------------------------------------------- #
# Hermetic mode (ARCH-FIX-SPEC.md R7, gate.sh's new fleet-tests stage)
# --------------------------------------------------------------------- #
#
# See test_ledger.py's copy of this same guard for the full rationale:
# `gate.sh` runs this suite as a hard-FAIL stage before the ratchet, and
# `TestCapabilityLedgerCheck`/`TestAcceptance` below build a release
# oxidex binary and shell to the real pinned oracle over the real
# combined-samples corpus -- 50-125s dominated by an uncached `cargo
# build --release` that gate.sh has no business paying twice (it already
# builds that binary a few lines before this stage runs).
HERMETIC_ENV = "FLEET_TESTS_HERMETIC"


def _hermetic() -> bool:
    return os.environ.get(HERMETIC_ENV) == "1"


def _not_hermetic() -> bool:
    return not _hermetic()


# --------------------------------------------------------------------- #
# Exemplar guard (ARCH-FIX-SPEC.md R7/T7 fixture refresh, 2026-08-15)
# --------------------------------------------------------------------- #
#
# `test_uncovered_format_is_not_a_hit` used to hard-code MRC as an
# "obviously still uncovered" format. 83bf5265 closed MRC's gap (MISSING
# 60->0) and the test kept asserting the opposite of measured reality --
# the suite was red at tip until this refresh, and nothing caught it
# because MRC was never re-measured, only assumed.
#
# See test_ledger.py's copy of this same guard (duplicated rather than
# imported, matching this file's existing convention of duplicating
# `_oracle_available`/`_corpus_available`/`_ensure_binary_built` rather
# than sharing a module between the two suites) for the measurement that
# picked DICOM: measured with `tools/fleet/ledger.py` against a release
# build of THIS tree on 2026-08-15 -- DICOM MISSING 92/101, MIFF MISSING
# 78/90, EIP MISSING 74/91; DICOM chosen for the largest margin.
EXEMPLAR_FORMAT = "DICOM"
EXEMPLAR_MISSING_THRESHOLD = 10  # generous margin under the measured 92


def _require_uncovered_exemplar():
    """Re-measure EXEMPLAR_FORMAT right now and skip loudly if it has been
    closed since this fixture was written, instead of asserting a premise
    that measured reality no longer supports (the exact MRC failure mode
    this refresh exists to fix).
    """
    binary = _ensure_binary_built()
    result = ledger.measure_format(REPO_ROOT, binary, ledger.ORACLE_SCRIPT, EXEMPLAR_FORMAT)
    if result.missing <= EXEMPLAR_MISSING_THRESHOLD:
        raise unittest.SkipTest(
            f"exemplar closed -- repoint me: {EXEMPLAR_FORMAT} now measures "
            f"MISSING={result.missing} (<= threshold {EXEMPLAR_MISSING_THRESHOLD}) under "
            f"tools/fleet/ledger.py against a release build. Pick a new still-uncovered "
            f"format (`python3 tools/fleet/ledger.py --repo . --format <X>`) and update "
            f"EXEMPLAR_FORMAT here."
        )
    return result


def _ensure_binary_built(timeout: int = 420):
    candidate = REPO_ROOT / "target" / "release" / "oxidex"
    if candidate.is_file():
        return candidate
    result = subprocess.run(
        ["cargo", "build", "--release", "--bin", "oxidex"],
        cwd=str(REPO_ROOT),
        capture_output=True,
        timeout=timeout,
    )  # nosec B603 B607
    if result.returncode != 0 or not candidate.is_file():
        raise RuntimeError(f"could not build oxidex --release: exit {result.returncode}")
    return candidate


class IntentTestCase(HermeticCase):
    """Base fixture: a throwaway bare repo standing in for the hub, exactly
    like `test_fleetlib.py`'s fixture (this file does not redefine it --
    T1.4 must build on `fleetlib`, not reinvent its test discipline).
    """

    def setUp(self):
        super().setUp()
        self._tmp_root = tempfile.mkdtemp(prefix="intent-test-")
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        self.workdir = str(Path(self._tmp_root) / "cache")

        init = _run_git(["git", "init", "--quiet", "--bare", self.hub_path])
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        resolved = str(Path(self.hub_path).resolve())
        system_tmp = str(Path(tempfile.gettempdir()).resolve())
        self.assertTrue(
            resolved.startswith(system_tmp),
            msg=f"test hub {resolved!r} is not under the system temp dir {system_tmp!r}",
        )
        self.assertNotIn("work2.oxidex.net", resolved)

        self.hub = make_hub(self, self.hub_path, workdir=self.workdir)

    def tearDown(self):
        shutil.rmtree(self._tmp_root, ignore_errors=True)

    def fresh_hub(self) -> Hub:
        other_workdir = tempfile.mkdtemp(prefix="intent-test-cache2-")
        self.addCleanup(shutil.rmtree, other_workdir, ignore_errors=True)
        return Hub(url=self.hub_path, workdir=other_workdir)


class TestFixtureGuard(IntentTestCase):
    def test_hub_url_is_a_temp_path(self):
        resolved = str(Path(self.hub.url).resolve())
        self.assertTrue(resolved.startswith(str(Path(tempfile.gettempdir()).resolve())))
        self.assertNotIn("work2.oxidex.net", resolved)


# --------------------------------------------------------------------- #
# Check 1: open-intent overlap (fast, no oracle/binary needed)
# --------------------------------------------------------------------- #


class TestOpenIntentOverlap(IntentTestCase):
    def test_no_open_intents_means_no_overlap(self):
        result = check_open_intent_overlap(self.hub, "new-slug", {"formats": ["XYZ"]})
        self.assertFalse(result.hit)

    def test_overlapping_format_is_caught(self):
        self.hub.create(
            intent_ref("existing"),
            {
                "slug": "existing",
                "title": "t",
                "scope": {"formats": ["MRC"], "tags": [], "files": []},
                "status": "open",
                "claimed_by": "host-a",
                "created_at": "2026-01-01T00:00:00+00:00",
            },
        )
        result = check_open_intent_overlap(self.hub, "new-slug", {"formats": ["mrc"], "tags": [], "files": []})
        self.assertTrue(result.hit)
        self.assertIn("existing", result.detail)

    def test_withdrawn_intent_does_not_count_as_overlap(self):
        self.hub.create(
            intent_ref("withdrawn-one"),
            {
                "slug": "withdrawn-one",
                "title": "t",
                "scope": {"formats": ["MRC"], "tags": [], "files": []},
                "status": "withdrawn",
                "claimed_by": "host-a",
                "created_at": "2026-01-01T00:00:00+00:00",
            },
        )
        result = check_open_intent_overlap(self.hub, "new-slug", {"formats": ["MRC"], "tags": [], "files": []})
        self.assertFalse(result.hit)

    def test_overlapping_file_glob_prefix_is_caught(self):
        self.hub.create(
            intent_ref("files-one"),
            {
                "slug": "files-one",
                "title": "t",
                "scope": {"formats": [], "tags": [], "files": ["src/parsers/raw/**"]},
                "status": "open",
                "claimed_by": "host-a",
                "created_at": "2026-01-01T00:00:00+00:00",
            },
        )
        result = check_open_intent_overlap(
            self.hub, "new-slug", {"formats": [], "tags": [], "files": ["src/parsers/raw/kyocera.rs"]}
        )
        self.assertTrue(result.hit)


# --------------------------------------------------------------------- #
# Check 2: history
# --------------------------------------------------------------------- #


class TestHistory(HermeticCase):
    def test_5cef5b3d_is_found_by_history_grep(self):
        # 5cef5b3d's commit body literally says "KyoceraRaw.raw" and
        # "SWF, PICT, PPM, RA" -- history is a cheap dedup signal and
        # SHOULD find this. (The acceptance test elsewhere confirms this is
        # not the reason intent.register cites, though -- see
        # TestAcceptance.)
        result = check_history(REPO_ROOT, {"formats": ["SWF"], "tags": []})
        self.assertTrue(result.hit)

    def test_novel_token_is_not_found(self):
        result = check_history(REPO_ROOT, {"formats": ["ZzyzxNoSuchFormatEverToken"], "tags": []})
        self.assertFalse(result.hit)


# --------------------------------------------------------------------- #
# Check 3: capability ledger wrapper
# --------------------------------------------------------------------- #


# HERMETIC SKIP: setUpClass builds the release oxidex binary and every
# test method shells to the real pinned oracle over the real corpus
# (via check_capability_ledger -> ledger.check_scope).
@unittest.skipUnless(_not_hermetic(), f"{HERMETIC_ENV}=1: skips real-binary/real-oracle/real-corpus tests")
@unittest.skipUnless(_oracle_available(), "pinned ExifTool oracle not usable in this environment")
@unittest.skipUnless(_corpus_available(), "combined-samples corpus not present in this environment")
class TestCapabilityLedgerCheck(HermeticCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        _ensure_binary_built()

    def test_empty_scope_is_not_a_hit(self):
        result = check_capability_ledger(REPO_ROOT, {"formats": [], "tags": []})
        self.assertFalse(result.hit)

    def test_covered_format_is_a_hit_citing_measured_evidence(self):
        result = check_capability_ledger(REPO_ROOT, {"formats": ["SWF"], "tags": []})
        self.assertTrue(result.hit)
        self.assertIn("MISSING 0", result.detail)

    def test_uncovered_format_is_not_a_hit(self):
        # Re-pointed from MRC (ARCH-FIX T7 fixture refresh, 2026-08-15):
        # 83bf5265 closed MRC's gap (MISSING 60->0) and this assertion went
        # stale, asserting the opposite of measured reality. See
        # _require_uncovered_exemplar above -- it re-measures DICOM now and
        # SKIPS loudly if it closes too, rather than repeating the same
        # failure with a new hard-coded name.
        _require_uncovered_exemplar()
        result = check_capability_ledger(REPO_ROOT, {"formats": [EXEMPLAR_FORMAT], "tags": []})
        self.assertFalse(result.hit)

    def test_broken_oracle_fails_closed_as_a_hit(self):
        # A ledger that cannot be trusted must refuse the WHOLE
        # registration rather than silently let it through -- see
        # check_capability_ledger's docstring.
        result = check_capability_ledger(
            REPO_ROOT, {"formats": ["SWF"], "tags": []}, ledger_kwargs={"oracle_script": Path("/nonexistent/oracle.sh")}
        )
        self.assertTrue(result.hit)
        self.assertIn("unusable", result.detail)


# --------------------------------------------------------------------- #
# THE ACCEPTANCE TEST
# --------------------------------------------------------------------- #


# HERMETIC SKIP: same reason as TestCapabilityLedgerCheck above -- THE
# non-negotiable acceptance test itself shells to the real oracle/binary
# via register() -> check_capability_ledger() for all five acceptance
# formats, plus a real `cargo build --release` in setUpClass.
@unittest.skipUnless(_not_hermetic(), f"{HERMETIC_ENV}=1: skips real-binary/real-oracle/real-corpus tests")
@unittest.skipUnless(_oracle_available(), "pinned ExifTool oracle not usable in this environment")
@unittest.skipUnless(_corpus_available(), "combined-samples corpus not present in this environment")
class TestAcceptance(IntentTestCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        _ensure_binary_built()

    def test_solo_ryzen5_intent_is_refused_by_capability_ledger(self):
        """THE non-negotiable acceptance test from the T1.4 task brief.

        Registering the `solo-ryzen5`/`route-legacy-formats` intent --
        SWF, PICT, PPM, RA, KyoceraRAW -- against the current tip must be
        REFUSED, because `5cef5b3d` already routed and measured all five.
        The refusal must cite the capability-ledger check specifically
        (with real MISSING counts), not merely the fact that a commit
        message happens to mention the same words.
        """
        result = register(
            self.hub,
            REPO_ROOT,
            slug="route-legacy-formats",
            title="Route SWF, PICT, PPM, RA, Kyocera RAW",
            scope={"formats": ["SWF", "PICT", "PPM", "RA", "KyoceraRAW"], "tags": [], "files": []},
            claimed_by="solo-ryzen5",
        )

        self.assertFalse(result.ok, "solo-ryzen5's intent registered successfully -- the ledger check is decorative")
        self.assertIsNotNone(result.reason)
        self.assertTrue(
            result.reason.startswith("[capability-ledger]"),
            f"refusal must be PRIMARILY attributed to the capability-ledger check, got: {result.reason!r}",
        )
        # Not just "a check named capability-ledger fired" -- its own
        # detail text must carry real measured numbers, proving this is a
        # behavioral verdict and not a text match wearing the right label.
        self.assertIn("MISSING 0", result.reason)
        self.assertIn("already covered at the tip", result.reason)

        # The history check must ALSO have fired independently (5cef5b3d's
        # message really does mention these tokens) -- but it must not be
        # what register() cites as the reason. This is the direct proof
        # that a naive history-grep-only implementation would have passed
        # this acceptance test for the wrong reason; ours does not rely on
        # it being the primary one.
        history_results = [c for c in result.checks if c.name == "history"]
        self.assertEqual(len(history_results), 1)
        self.assertTrue(history_results[0].hit, "expected history to ALSO independently flag this (sanity check)")

        ledger_results = [c for c in result.checks if c.name == "capability-ledger"]
        self.assertEqual(len(ledger_results), 1)
        self.assertTrue(ledger_results[0].hit)
        for fmt in ("SWF", "PICT", "PPM", "RA", "KyoceraRAW"):
            self.assertIn(fmt, ledger_results[0].detail)

        # And the ref must genuinely not exist on the hub.
        self.assertIsNone(self.hub.sha(intent_ref("route-legacy-formats")))

    def test_genuinely_new_format_registers_fine(self):
        # THE SAME GUARD AS `_require_uncovered_exemplar`, applied to this
        # test's own format. This test hard-codes ZISRAW rather than using
        # EXEMPLAR_FORMAT because it needs a format that is BOTH an open
        # gap AND unmentioned in tip history (see the comment below), and
        # DICOM -- the exemplar -- fails the second half now that
        # staging/cov2-dicom has landed. Hard-coding it re-created the exact
        # MRC failure mode the refresh above exists to fix: the coverage
        # line closed ZISRAW (src/parsers/image/czi.rs, 146 -> 480 lines via
        # staging/cov-wave-3) while this fixture went on asserting the gap
        # was open, and the two only met at integration -- neither branch is
        # red alone. Re-measure and skip loudly rather than assert a premise
        # measured reality no longer supports.
        _open_gap = ledger.measure_format(
            REPO_ROOT, _ensure_binary_built(), ledger.ORACLE_SCRIPT, "ZISRAW")
        if _open_gap.missing <= EXEMPLAR_MISSING_THRESHOLD:
            raise unittest.SkipTest(
                f"ZISRAW is no longer an open gap -- repoint me: it now measures "
                f"MISSING={_open_gap.missing} (<= threshold "
                f"{EXEMPLAR_MISSING_THRESHOLD}) under tools/fleet/ledger.py against a "
                f"release build. This test needs a format that is BOTH still "
                f"uncovered AND unmentioned in tip commit history (so check_history "
                f"stays clean and the ledger is isolated); find one with "
                f"`python3 tools/fleet/ledger.py --repo . --format <X>` and replace "
                f"ZISRAW throughout this test."
            )

        # ZISRAW is a REAL, currently-open gap: 31 of 34 comparable tags
        # are MISSING against the pinned oracle on ZISRAW.czi (verified by
        # hand while building this suite -- see the T1.4 report), and no
        # commit on the tip mentions it, so `check_history` also stays
        # clean. (MRC was tried first and rejected as the example here: it
        # IS a real gap too, but c2c54c40's message already says "route
        # ... MRC", so `check_history` correctly flags it independently of
        # the ledger -- a good demonstration that history is a real check,
        # a bad choice for a test that wants to isolate the ledger.) An
        # intent to close a clean gap like ZISRAW's must be allowed through.
        result = register(
            self.hub,
            REPO_ROOT,
            slug="close-zisraw-gap",
            title="Close the ZISRAW tag-coverage gap",
            scope={"formats": ["ZISRAW"], "tags": [], "files": []},
            claimed_by="some-host",
        )
        self.assertTrue(result.ok, result.reason)
        self.assertIsNotNone(self.hub.sha(intent_ref("close-zisraw-gap")))
        payload = self.hub.read(intent_ref("close-zisraw-gap"))
        self.assertEqual(payload["status"], "open")
        self.assertEqual(payload["scope"]["formats"], ["ZISRAW"])


# --------------------------------------------------------------------- #
# Real concurrency: exactly one racer wins a slug
# --------------------------------------------------------------------- #


def _race_worker(spec: dict, workdir: str, repo_root: str, slug: str, claimed_by: str) -> bool:
    """Runs in a child process started with `_MP_CONTEXT` (`tests/_mp.py`).

    Uses a scope with no formats/tags -- files-only -- so this test
    isolates the property under test (the hub ref's create-only CAS) from
    needing a live oracle/binary in every worker, while still exercising
    the SAME `register()` code path the acceptance test uses, including
    all three real checks.

    `spec` comes from `_fixtures.hub_spec`, so each racer builds the hub
    shape the parent's fixture mode dictates: a plain `Hub` in bare mode,
    `FallbackHub(ServerHub, Hub)` through the fixture keel-server under
    `FLEET_TEST_HUB=server` -- the race then contends on the server's CAS,
    not around it.
    """
    sys.path.insert(0, str(Path(repo_root) / "tools" / "fleet"))
    sys.path.insert(0, str(Path(repo_root) / "tools" / "fleet" / "tests"))
    from _fixtures import hub_from_spec as _hub_from_spec  # local import: see note above
    from intent import register as _register

    hub = _hub_from_spec(spec, workdir)
    result = _register(
        hub,
        Path(repo_root),
        slug=slug,
        title="race target",
        scope={"formats": [], "tags": [], "files": [f"src/nonexistent/{claimed_by}/**"]},
        claimed_by=claimed_by,
    )
    return result.ok


class TestConcurrentRegistration(IntentTestCase):
    def test_two_racers_same_slug_exactly_one_wins(self):
        n = 6
        spec = hub_spec(self.hub, self.hub_path)
        with ProcessPoolExecutor(max_workers=n, mp_context=_MP_CONTEXT) as pool:
            futures = [
                pool.submit(_race_worker, spec, tempfile.mkdtemp(prefix=f"race-cache-{i}-"), str(REPO_ROOT), "race-slug", f"host-{i}")
                for i in range(n)
            ]
            outcomes = [f.result(timeout=60) for f in as_completed(futures)]

        self.assertEqual(sum(1 for ok in outcomes if ok), 1, f"expected exactly one winner, got {outcomes}")
        self.assertIsNotNone(self.hub.sha(intent_ref("race-slug")))

    def test_two_racers_different_slugs_both_win(self):
        n = 4
        spec = hub_spec(self.hub, self.hub_path)
        with ProcessPoolExecutor(max_workers=n, mp_context=_MP_CONTEXT) as pool:
            futures = [
                pool.submit(
                    _race_worker,
                    spec,
                    tempfile.mkdtemp(prefix=f"race-cache-distinct-{i}-"),
                    str(REPO_ROOT),
                    f"distinct-slug-{i}",
                    f"host-{i}",
                )
                for i in range(n)
            ]
            outcomes = [f.result(timeout=60) for f in as_completed(futures)]
        self.assertTrue(all(outcomes), outcomes)


# --------------------------------------------------------------------- #
# withdraw() / list_open_intents()
# --------------------------------------------------------------------- #


class TestWithdrawAndList(IntentTestCase):
    def test_withdraw_marks_status_and_removes_from_open_list(self):
        result = register(
            self.hub,
            REPO_ROOT,
            slug="temp-intent",
            title="t",
            scope={"formats": [], "tags": [], "files": ["src/some/path/**"]},
            claimed_by="host-a",
        )
        self.assertTrue(result.ok, result.reason)
        self.assertIn("temp-intent", list_open_intents(self.hub))

        self.assertTrue(withdraw(self.hub, "temp-intent"))
        self.assertNotIn("temp-intent", list_open_intents(self.hub))
        payload = self.hub.read(intent_ref("temp-intent"))
        self.assertEqual(payload["status"], "withdrawn")

    def test_withdraw_of_absent_slug_returns_false(self):
        self.assertFalse(withdraw(self.hub, "never-registered"))


if __name__ == "__main__":
    unittest.main()
