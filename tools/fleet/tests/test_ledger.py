#!/usr/bin/env python3
"""Tests for tools/fleet/ledger.py -- the capability ledger.

Unlike `test_fleetlib.py`/`test_claim.py`, most of this file deliberately
does NOT run against a throwaway fixture: the capability ledger's entire
job is to measure the REAL binary, built from THIS checkout, against the
REAL pinned ExifTool oracle. Faking any of those three would test nothing
-- see `ledger.py`'s module docstring on why a text-matching stub dressed
up as a capability check is exactly the failure this task must not ship.

The one thing tests here DO fake, deliberately, is the *corpus* for
`measure_tag` (a temp directory holding a single copy of one real sample)
-- purely so a tag-level test doesn't have to scan the whole ~200-file
corpus to find one tag. The oracle and the binary invoked against that
sample are still the real ones.

Ground rule (T1.4 task brief): never invoke bare `exiftool`, only the
pinned wrapper, and only after capability-probing it. Every test here that
needs the oracle goes through `ledger.probe_capability` first and SKIPS
(never silently passes) if the fixed environment paths this task assumes
(`/tmp/oxidex-exiftool-cache/...`) are not present -- this suite is not
the place to stand up that infrastructure.

Plain `unittest`, standard library only (no pytest in this environment).

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import ledger  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]


def _oracle_available() -> bool:
    probe = ledger.probe_capability()
    return probe.ok


def _corpus_available() -> bool:
    return ledger.CORPUS_DIR.is_dir()


def _ensure_binary_built(timeout: int = 420) -> Path:
    """The release oxidex binary for THIS checkout, building it if a fresh
    clone doesn't have one yet. Not a fixture -- the ledger's subject is
    this repo's own binary, so there is nothing to fake here.
    """
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
        raise RuntimeError(
            "could not build oxidex --release for the ledger test suite: "
            f"exit {result.returncode}, stderr tail: "
            f"{result.stderr.decode('utf-8', 'replace')[-2000:]}"
        )
    return candidate


@unittest.skipUnless(_oracle_available(), "pinned ExifTool oracle not usable in this environment")
@unittest.skipUnless(_corpus_available(), "combined-samples corpus not present in this environment")
class LedgerTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = _ensure_binary_built()


class TestCapabilityProbe(unittest.TestCase):
    def test_real_pinned_oracle_probes_ok(self):
        probe = ledger.probe_capability()
        if not probe.ok:
            self.skipTest(f"pinned oracle not usable here: {probe.detail}")
        self.assertEqual(probe.oracle_version, ledger.EXPECTED_ORACLE_VERSION)
        self.assertEqual(probe.docx_filetype, "DOCX")

    def test_missing_oracle_script_fails_closed(self):
        probe = ledger.probe_capability(oracle_script=Path("/nonexistent/exiftool-pinned.sh"))
        self.assertFalse(probe.ok)
        self.assertIn("missing", probe.detail)

    def test_wrong_version_fails_closed(self):
        # A tiny fake "oracle" that always prints the wrong version --
        # proves the probe actually reads and checks the version rather
        # than assuming success from a zero exit code.
        with tempfile.TemporaryDirectory() as td:
            fake = Path(td) / "fake-oracle.sh"
            fake.write_text("#!/bin/sh\necho 13.55\n")
            fake.chmod(0o755)
            probe = ledger.probe_capability(oracle_script=fake, expected_version="13.59")
            self.assertFalse(probe.ok)
            self.assertIn("13.55", probe.detail)

    def test_docx_probe_failure_is_distinguished_from_version_failure(self):
        # Simulates the exact documented failure mode: -ver matches, but the
        # capability probe does not. A ledger that only checked -ver would
        # pass this; probe_capability must not.
        with tempfile.TemporaryDirectory() as td:
            fake = Path(td) / "fake-oracle.sh"
            fake.write_text(
                "#!/bin/sh\n"
                'if [ "$1" = "-ver" ]; then echo 13.59; else echo ZIP; fi\n'
            )
            fake.chmod(0o755)
            docx = Path(td) / "OOXML.docx"
            docx.write_bytes(b"not a real docx, just needs to exist")
            probe = ledger.probe_capability(oracle_script=fake, docx_sample=docx, expected_version="13.59")
            self.assertFalse(probe.ok)
            self.assertEqual(probe.oracle_version, "13.59")
            self.assertIn("ZIP", probe.detail)
            self.assertIn("degraded", probe.detail.lower())


class TestNormalizeToken(unittest.TestCase):
    def test_case_and_punctuation_insensitive(self):
        self.assertEqual(ledger.normalize_token("KyoceraRAW"), "KYOCERARAW")
        self.assertEqual(ledger.normalize_token("kyocera-raw"), "KYOCERARAW")
        self.assertEqual(ledger.normalize_token("  swf "), "SWF")


@unittest.skipUnless(_corpus_available(), "combined-samples corpus not present in this environment")
class TestFindSampleForFormat(unittest.TestCase):
    def test_extension_match(self):
        for fmt, expected in (("SWF", "Flash.swf"), ("PICT", "PICT.pict"), ("PPM", "PPM.ppm"), ("RA", "Real.ra"), ("MRC", "MRC.mrc")):
            with self.subTest(fmt=fmt):
                sample = ledger.find_sample_for_format(fmt)
                self.assertIsNotNone(sample, f"no sample found for {fmt}")
                self.assertEqual(sample.name, expected)

    def test_stem_match_for_kyocera_raw(self):
        # KyoceraRaw.raw's extension ("raw") does NOT equal the normalized
        # token ("KYOCERARAW") -- this specifically exercises the stem
        # fallback, not the extension fast path.
        sample = ledger.find_sample_for_format("KyoceraRAW")
        self.assertIsNotNone(sample)
        self.assertEqual(sample.name, "KyoceraRaw.raw")

    def test_override_for_shared_netpbm_filetype(self):
        sample = ledger.find_sample_for_format("PGM")
        self.assertIsNotNone(sample)
        self.assertEqual(sample.name, "PPM.ppm")

    def test_unknown_format_returns_none(self):
        self.assertIsNone(ledger.find_sample_for_format("TotallyMadeUpFormatXyz"))


class TestResolveOxidexBinary(unittest.TestCase):
    def test_missing_binary_raises_ledger_error(self):
        with tempfile.TemporaryDirectory() as td:
            empty_repo = Path(td)
            (empty_repo / "target").mkdir()
            with self.assertRaises(ledger.LedgerError):
                ledger.resolve_oxidex_binary(empty_repo)

    def test_override_to_nonexistent_path_raises(self):
        with self.assertRaises(ledger.LedgerError):
            ledger.resolve_oxidex_binary(REPO_ROOT, override="/nonexistent/oxidex")

    def test_resolves_real_release_binary(self):
        binary = _ensure_binary_built()
        resolution = ledger.resolve_oxidex_binary(REPO_ROOT)
        self.assertEqual(resolution.path, binary.resolve())
        self.assertEqual(resolution.candidate, "target/release/oxidex")


class TestGrepDispatchEvidenceIsNeverAuthoritativeAlone(unittest.TestCase):
    """Locks in the exact trap `measure_format` exists to avoid: KyoceraRAW
    has no `FileFormat::KyoceraRAW` token in format_dispatch.rs at all.
    """

    def test_kyocera_raw_has_no_format_dispatch_hit(self):
        hits = ledger.grep_dispatch_evidence(REPO_ROOT, "KyoceraRAW")
        self.assertNotIn("src/core/format_dispatch.rs", hits)

    def test_kyocera_raw_is_found_via_detection_module(self):
        hits = ledger.grep_dispatch_evidence(REPO_ROOT, "Kyocera")
        self.assertIn("src/parsers/detection/mod.rs", hits)


class TestMeasureFormat(LedgerTestCase):
    def test_swf_is_fully_covered(self):
        result = ledger.measure_format(REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, "SWF")
        self.assertTrue(result.covered, result.reason)
        self.assertEqual(result.missing, 0)
        self.assertGreater(result.compared, 0)

    def test_all_five_acceptance_formats_are_fully_covered(self):
        # This is the ledger-level half of the non-negotiable acceptance
        # test: SWF, PICT, PPM, RA, KyoceraRAW must all independently
        # measure as covered, proving intent.py's refusal (tested in
        # test_intent.py) traces back to real per-format evidence and not
        # a lucky aggregate.
        for fmt in ("SWF", "PICT", "PPM", "RA", "KyoceraRAW"):
            with self.subTest(fmt=fmt):
                result = ledger.measure_format(REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, fmt)
                self.assertTrue(result.covered, result.reason)
                self.assertEqual(result.missing, 0, result.reason)
                self.assertGreater(result.compared, 0)

    def test_kyocera_raw_is_covered_despite_no_format_dispatch_grep_hit(self):
        # The direct proof this check is behavioral, not textual: the same
        # format that has zero format_dispatch.rs hits (see
        # TestGrepDispatchEvidenceIsNeverAuthoritativeAlone) still measures
        # as fully covered, because coverage was decided by running the
        # binary, not by grepping for the format's name.
        result = ledger.measure_format(REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, "KyoceraRAW")
        self.assertNotIn("src/core/format_dispatch.rs", result.dispatch_hits)
        self.assertTrue(result.covered, result.reason)
        self.assertEqual(result.missing, 0)

    def test_a_currently_uncovered_format_measures_not_covered(self):
        # MRC has a real dispatched parser (`FileFormat::MRC` IS present in
        # format_dispatch.rs -- a grep-only check would call it covered)
        # but the parser only implements a fraction of ExifTool's MRC.pm
        # table, so a real diff finds a large MISSING count. This is the
        # mirror-image proof to the KyoceraRAW case above: presence in
        # format_dispatch.rs is neither necessary NOR sufficient for
        # "covered" -- only the measured MISSING count decides it.
        #
        # NOTE: if a future commit closes the MRC gap, this test's
        # `assertFalse` will need to move to a still-open format -- that is
        # the correct outcome of a real fix landing, not a flake.
        result = ledger.measure_format(REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, "MRC")
        self.assertIn("src/core/format_dispatch.rs", result.dispatch_hits)
        self.assertFalse(result.covered, result.reason)
        self.assertGreater(result.missing, 0)

    def test_format_with_no_corpus_sample_is_not_covered(self):
        result = ledger.measure_format(REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, "TotallyMadeUpFormatXyz")
        self.assertIsNone(result.sample)
        self.assertFalse(result.covered)
        self.assertIn("no corpus sample", result.reason)


class TestMeasureTag(LedgerTestCase):
    """Uses a filtered one-file corpus purely to keep the scan bounded and
    deterministic (no tag->file index exists to look this up directly) --
    the oracle and binary invocations underneath are still real.
    """

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls._tmp = tempfile.mkdtemp(prefix="ledger-tag-corpus-")
        cls.corpus_dir = Path(cls._tmp)
        shutil.copy(ledger.CORPUS_DIR / "MRC.mrc", cls.corpus_dir / "MRC.mrc")

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls._tmp, ignore_errors=True)

    def test_tag_oxidex_emits_is_covered(self):
        # CellAlpha is one of MRC's tags oxidex DOES emit (see the T1.4
        # report for how this pair was found).
        result = ledger.measure_tag(REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, "MRC:CellAlpha", corpus_dir=self.corpus_dir)
        self.assertTrue(result.covered, result.reason)
        self.assertIsNotNone(result.sample)

    def test_tag_oxidex_does_not_emit_is_not_covered(self):
        result = ledger.measure_tag(REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, "MRC:AlphaTilt", corpus_dir=self.corpus_dir)
        self.assertFalse(result.covered, result.reason)
        self.assertIsNotNone(result.sample)

    def test_tag_never_seen_in_scanned_corpus_is_not_covered(self):
        result = ledger.measure_tag(
            REPO_ROOT, self.binary, ledger.ORACLE_SCRIPT, "NoSuchTagEverXyz", corpus_dir=self.corpus_dir
        )
        self.assertFalse(result.covered)
        self.assertIsNone(result.sample)
        self.assertIn("not observed", result.reason)


class TestCheckScope(LedgerTestCase):
    def test_degraded_oracle_raises_rather_than_reporting_covered(self):
        # THE dangerous failure mode named throughout ledger.py's docstring:
        # a broken/degraded oracle must never be silently read as "nothing
        # is MISSING, so everything is covered." check_scope must raise.
        with tempfile.TemporaryDirectory() as td:
            broken_oracle = Path(td) / "broken.sh"
            broken_oracle.write_text("#!/bin/sh\necho not-a-version\n")
            broken_oracle.chmod(0o755)
            with self.assertRaises(ledger.LedgerError):
                ledger.check_scope(REPO_ROOT, {"formats": ["SWF"]}, oracle_script=broken_oracle)

    def test_acceptance_scope_is_fully_covered_with_measured_reasons(self):
        scope = {"formats": ["SWF", "PICT", "PPM", "RA", "KyoceraRAW"], "tags": [], "files": []}
        report = ledger.check_scope(REPO_ROOT, scope)
        self.assertTrue(report.already_covered)
        self.assertEqual(len(report.covered_reasons()), 5)
        for reason in report.covered_reasons():
            self.assertIn("MISSING 0", reason)
            self.assertIn("measured", reason)


if __name__ == "__main__":
    unittest.main()
