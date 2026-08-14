#!/usr/bin/env python3
"""Non-negotiable acceptance fixtures for tools/fleet/verdict.py (T1.2).

Replays three incidents that actually happened, as minimal synthetic git
repos with the same shape (same file paths, same overlap pattern) as the
real ones. Every one of the three branches involved was individually green
-- that's the whole point: `is_admissible` must reject each cached verdict
anyway, once the tip moved out from under it, because file-disjointness
between branches never proved they were safe to combine.

  1. Census break -- two table-wiring branches (`APE::NewHeader`,
     `H264::RecInfo`, named for the real ones in
     `src/exiftool_tables/mod.rs`'s own test-history comment), each green
     alone, each independently bumping `enabled.rs` (a declared conflict
     domain) to register its table. Rejected via the branch's *own*
     write_set touching a domain -- `conflict-domain-branch`.

  2. `verify_subdirs.py` break -- a branch green alone whose own change has
     nothing to do with `verify_subdirs.py`, merged after an unrelated
     "Step 25" commit changed that file's tuple width. Rejected via an
     *intervening* commit touching a domain the branch never touched
     itself -- `conflict-domain-intervening`.

  3. Generated-enum break -- three individually-PASSing branches: one
     regenerates `binary_tables.rs`, one hand-edits `runtime.rs`'s
     `ExprId` references, and a third is unrelated to both (a plain
     parser fix, file-disjoint from *everything* in this story). The
     third branch's cached verdict is the one under test here, and it is
     rejected purely because two *other* branches' commits landed on two
     declared domains in between -- `conflict-domain-intervening` again,
     this time with two domain files and zero write-set overlap of any
     kind, which is the sharpest demonstration of why file-disjointness
     alone is not sufficient.

Each test also runs the identical check with `domains=frozenset()` (i.e.
without `fleet/domains.toml`'s knowledge) and asserts THAT call returns
admissible -- proving the domain declaration, not some other detail of the
fixture, is what's catching the collision. If `is_admissible` admitted any
of these three with the real domain set loaded, the mechanism would not be
real and this file must fail loudly rather than be adjusted to pass.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from gitfixture import RepoBuilder, require_temp_path  # noqa: E402
import verdict  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
DOMAINS_TOML = REPO_ROOT / "fleet" / "domains.toml"

PLATFORM_ID = "platform-under-test"


def _verdict(**overrides) -> dict:
    payload = {
        "tree_sha": "t" * 40,
        "base_tip": None,  # filled in per fixture
        "branch": "staging/example",
        "result": "PASS",
        "stage": "complete",
        "gate_version": "2",
        "rustc_id": "r" * 64,
        "platform_id": PLATFORM_ID,
        "host": "server",
        "duration_s": 900,
        "write_set": [],
    }
    payload.update(overrides)
    return payload


class FixtureTestCase(unittest.TestCase):
    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="verdict-fixture-")
        require_temp_path(self._tmp_root)
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.repo = RepoBuilder(Path(self._tmp_root) / "repo")
        self.real_domains = verdict.load_domains(DOMAINS_TOML)

    def assert_domains_are_the_differentiator(self, verdict_payload, current_tip):
        """The load-bearing assertion: reject *because of* the domain
        declaration, not by accident of some other detail of the fixture.
        """
        without_domains = verdict.is_admissible(
            verdict_payload,
            current_tip,
            repo=self.repo.path,
            target_platform_id=PLATFORM_ID,
            domains=frozenset(),
        )
        self.assertTrue(
            without_domains.admissible,
            f"fixture is not actually isolated by domain knowledge -- without domains.toml it "
            f"should have been admitted (proving the domain declaration is what catches it), "
            f"but got: {without_domains}",
        )


class TestCensusBreak(FixtureTestCase):
    """Incident 1: `assert_eq!(enabled, 5)` broke after two independently
    green table-wiring branches (APE::NewHeader, H264::RecInfo) each
    bumped `src/exiftool_tables/enabled.rs`.
    """

    def test_second_branch_own_stale_pass_is_rejected(self):
        base_tip = self.repo.commit(
            {
                "src/exiftool_tables/mod.rs": "assert_eq!(enabled, 5);\n",
                "src/exiftool_tables/enabled.rs": "pub const ENABLED: &[&str] = &[/* 5 entries */];\n",
            },
            "T0: baseline census at 5",
        )

        # APE::NewHeader merges first -- also green alone, also bumps the
        # allowlist -- and becomes the new tip.
        current_tip = self.repo.commit(
            {"src/exiftool_tables/enabled.rs": "pub const ENABLED: &[&str] = &[/* 6 entries, +APE::NewHeader */];\n"},
            "merge staging/wire-ape-newheader",
        )

        # H264::RecInfo was gated PASS back when the tip was still T0 --
        # before APE::NewHeader landed -- and its own change also touches
        # the allowlist to register its table.
        h264_verdict = _verdict(
            branch="staging/wire-h264-recinfo",
            base_tip=base_tip,
            write_set=["src/exiftool_tables/tables/h264_recinfo.rs", "src/exiftool_tables/enabled.rs"],
        )

        result = verdict.is_admissible(
            h264_verdict,
            current_tip,
            repo=self.repo.path,
            target_platform_id=PLATFORM_ID,
            domains=self.real_domains,
        )

        self.assertFalse(result.admissible, "the census-break fixture must be rejected, not admitted")
        self.assertEqual(result.reason, "conflict-domain-branch")
        self.assertIn("enabled.rs", result.detail)

        # Unlike the other two fixtures, this one is *not* a clean
        # demonstration that domains.toml alone is the differentiator: per
        # the real `src/exiftool_tables/mod.rs` comment this replays, both
        # branches literally edit `enabled.rs` to register their table, so
        # even a domain-naive write-set-overlap check independently catches
        # it (the two branches were never file-disjoint to begin with).
        # Assert that explicitly, rather than claiming a stronger property
        # this fixture doesn't actually have.
        without_domains = verdict.is_admissible(
            h264_verdict, current_tip, repo=self.repo.path, target_platform_id=PLATFORM_ID, domains=frozenset()
        )
        self.assertFalse(
            without_domains.admissible,
            "this fixture's two branches share a literal file (enabled.rs), so plain "
            "write-set-overlap should reject it even with no domain knowledge at all",
        )
        self.assertEqual(without_domains.reason, "write-set-overlap")


class TestVerifySubdirsBreak(FixtureTestCase):
    """Incident 2: `verify_subdirs.py:208` unpacked 8 values from an
    11-tuple after a branch merged behind "Step 25"'s tuple-width change,
    killing `just verify-tables`. The victim branch's own change has
    nothing to do with `verify_subdirs.py` -- that's what makes this the
    sharper case than the census one.
    """

    def test_stale_pass_merged_after_step25_is_rejected(self):
        base_tip = self.repo.commit(
            {
                "tools/exiftool-tables/verify_subdirs.py": "a, b, c, d, e, f, g, h = record\n",
                "src/exiftool_tables/subdir.rs": "// baseline subdir edges\n",
            },
            "T0: baseline, 8-tuple unpack",
        )

        # Step 25 lands on the tip: widens the tuple to 11 fields.
        current_tip = self.repo.commit(
            {
                "tools/exiftool-tables/verify_subdirs.py": (
                    "a, b, c, d, e, f, g, h, i, j, k = record  # Step 25: 11-tuple\n"
                )
            },
            "Step 25: widen verify_subdirs.py's record tuple",
        )

        # A completely unrelated branch -- new SubDirectory routing edges
        # for a different module -- was gated PASS at T0, before Step 25.
        routing_verdict = _verdict(
            branch="staging/subdir-edges-fuji",
            base_tip=base_tip,
            write_set=["src/exiftool_tables/tables/fuji_subdirs.rs"],
        )

        result = verdict.is_admissible(
            routing_verdict,
            current_tip,
            repo=self.repo.path,
            target_platform_id=PLATFORM_ID,
            domains=self.real_domains,
        )

        self.assertFalse(result.admissible, "the verify_subdirs.py fixture must be rejected, not admitted")
        self.assertEqual(result.reason, "conflict-domain-intervening")
        self.assertIn("verify_subdirs.py", result.detail)

        self.assert_domains_are_the_differentiator(routing_verdict, current_tip)


class TestGeneratedEnumBreak(FixtureTestCase):
    """Incident 3 (today's): merging three individually-PASSing branches
    produced `error[E0599]: no variant named 'Sprintf0fValB74070' found
    for enum 'ExprId'` at `src/exiftool_tables/runtime.rs:1094`, because a
    hand-written reference in `runtime.rs` and the GENERATED
    `binary_tables.rs` came from different branches.

    The branch under test here is the *third* one -- entirely unrelated to
    the enum, file-disjoint from both of the other two and from the
    domains themselves. It is rejected purely because two other branches'
    commits landed on two declared domains while it waited in the queue.
    """

    def test_third_unrelated_branch_own_stale_pass_is_rejected(self):
        base_tip = self.repo.commit(
            {
                "src/exiftool_tables/binary_tables.rs": "// GENERATED: ExprId variants (old set)\n",
                "src/exiftool_tables/runtime.rs": "// hand-written: matches old ExprId set\n",
            },
            "T0: baseline generated enum + runtime references",
        )

        # Branch R: regenerates binary_tables.rs, adding new variants
        # including Sprintf0fValB74070.
        after_regen = self.repo.commit(
            {
                "src/exiftool_tables/binary_tables.rs": (
                    "// GENERATED: ExprId variants (new set, incl. Sprintf0fValB74070)\n"
                )
            },
            "merge staging/regen-binary-tables",
        )

        # Branch H: hand-edits runtime.rs to add a reference that only
        # makes sense paired with R's regeneration.
        current_tip = self.repo.commit(
            {"src/exiftool_tables/runtime.rs": "// hand-written: references Sprintf0fValB74070\n"},
            "merge staging/runtime-sprintf-ref",
        )

        # Branch X: a plain parser fix, gated PASS back at T0, sharing not
        # one file with R, H, or either domain.
        unrelated_verdict = _verdict(
            branch="staging/parser-fix-unrelated",
            base_tip=base_tip,
            write_set=["src/parsers/some_format.rs"],
        )

        result = verdict.is_admissible(
            unrelated_verdict,
            current_tip,
            repo=self.repo.path,
            target_platform_id=PLATFORM_ID,
            domains=self.real_domains,
        )

        self.assertFalse(
            result.admissible,
            "the generated-enum fixture must be rejected even though the victim branch "
            "shares zero files with either offending branch",
        )
        self.assertEqual(result.reason, "conflict-domain-intervening")
        self.assertTrue(
            "binary_tables.rs" in result.detail or "runtime.rs" in result.detail,
            f"expected the rejection detail to name a generated-enum domain file, got: {result.detail}",
        )

        self.assert_domains_are_the_differentiator(unrelated_verdict, current_tip)


if __name__ == "__main__":
    unittest.main()
