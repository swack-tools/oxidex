"""Pins the GENERATED-FILE doctrine carried in agentworker's worker prompts.

WHY THIS FILE EXISTS
--------------------
`agentworker.build_prompt` hands a fleet worker the rule for resolving
`src/exiftool_tables/binary_tables.rs` during a merge. That rule has been
wrong twice, and both times the damage was silent -- the agent did exactly
what it was told, the merge committed clean, and nothing failed until a
verified regeneration had already been overwritten.

The prompt is a bare f-string. Nothing imported it, nothing rendered it,
nothing asserted on it; a wrong rule sat in it across two landings. That
is the gap this module closes: it renders the REAL prompts and asserts on
the load-bearing substrings.

Substrings, not a golden file. A whole-text snapshot would go red on every
unrelated wording tweak and be deleted inside a month; these needles are
the clauses whose ABSENCE is the defect.

Both halves matter, and the second is the one that catches a regression:

  * present-checks fail if the content-conditional rule is deleted;
  * absent-checks fail if either defective ancestor is restored -- including
    the case where someone APPENDS the old rule back alongside the new one,
    which every present-check would happily survive.

THE TWO DEFECTIVE ANCESTORS (specimens embedded below, with citations)

  1. UNCONDITIONAL take-the-tip -- `66eb3ea5` through `09e30f75`.
     "always take the tip's version verbatim". This is the one that reset a
     verified regen and cost a force-push recovery.

  2. CONFLICT-conditional -- `6b31f38e` through `5c3df01f^`. A partial fix:
     it gated on whether git reported a CONFLICT rather than on content, so
     a textual auto-merge of two regens was taken silently. That is exactly
     the chimera the current rule names.

  Fixed in `5c3df01f` (land agent-intent-crashfix), which made the rule
  CONTENT-conditional: capture $BASE/$BRANCH_BEFORE, decide by
  `git diff <base> <tip> -- <path>`, and verify the result by content on
  both arms.

A TRAP WORTH NAMING: the CORRECT prompt legitimately contains the phrase
"take the tip's version verbatim" -- on the tip-CHANGED arm, where taking
the tip IS right. Only the leading "always" distinguishes the unconditional
defect from the correct conditional clause, so the forbidden needle must
carry it. `test_forbidden_needles_match_the_real_defect_specimens` proves
each needle still fires on the historical text, so these are not needles
that can no longer match anything.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import agentworker  # noqa: E402
from _env import HermeticCase  # noqa: E402

# Representative arguments, mirroring what agentworker.run() actually passes.
TIP_SHA = "0f1e2d3c4b5a69788796a5b4c3d2e1f001234567"
HUB_URL = "git@github.com:swack-tools/oxidex.git"
BRANCH = "staging/cov-example"
HOST = "m5"
SLUG = "cov-aiff-samplerate"
INTENT = {
    "title": "AIFF: close 21 MISSING under conformance.py",
    "scope": {"formats": ["AIFF", "AIFC"]},
}

GENERATED = "src/exiftool_tables/binary_tables.rs"


def _around(haystack: str, needle: str, pad: int = 90) -> str:
    """The neighbourhood of a match, so a failure names WHERE, not just what."""
    i = haystack.find(needle)
    if i < 0:
        return "(not found)"
    return haystack[max(0, i - pad):i + len(needle) + pad].replace("\n", " ")


class ClauseAssertions(HermeticCase):
    """assertIn/assertNotIn print the entire multi-KB prompt before the
    message, so the one line a reader needs scrolls off. These report the
    clause and nothing else."""

    def assertClause(self, prompt: str, clause: str, label: str = ""):
        if clause not in prompt:
            self.fail(f"missing doctrine clause{' in ' + label if label else ''}:\n"
                      f"  expected: {clause!r}\n"
                      f"  This clause is load-bearing -- see the module docstring "
                      f"for what its absence cost.")

    def assertNoClause(self, prompt: str, clause: str, label: str = ""):
        if clause in prompt:
            self.fail(f"forbidden clause present{' in ' + label if label else ''}:\n"
                      f"  found:   {clause!r}\n"
                      f"  context: ...{_around(prompt, clause)}...")


# --- defect specimens -------------------------------------------------------
# Verbatim source lines from the two bad ancestors, kept so the forbidden
# needles below can be proven to still match the thing they exist to catch.
# Retrieved with `git show <sha>:tools/fleet/agentworker.py`. The `{tip_sha}`
# in them is the un-rendered f-string placeholder, harmless under substring
# search.

DEFECT_UNCONDITIONAL = (
    "   - `src/exiftool_tables/binary_tables.rs` is GENERATED: always take the tip's "
    "version verbatim (`git checkout --theirs` semantics depend on merge direction -- "
    "VERIFY the result matches the tip by content: `git diff {tip_sha} -- "
    "src/exiftool_tables/binary_tables.rs` must be empty). Never hand-edit it, never "
    "invent enum variants."
)  # 66eb3ea5 .. 09e30f75

DEFECT_CONFLICT_CONDITIONAL = (
    "   - `src/exiftool_tables/binary_tables.rs` is GENERATED: never hand-edit it, "
    "never invent enum variants. Resolve it to the tip's version ONLY IF THE MERGE "
    "CONFLICTS ON IT. If the merge does not touch it, LEAVE IT ALONE -- a branch may "
    "legitimately carry freshly REGENERATED tables (its commits will say so)"
)  # 6b31f38e .. 5c3df01f^

# --- forbidden needles ------------------------------------------------------
# Each must be absent from every rendered prompt. Keyed to the specimens above.
FORBIDDEN = {
    # Ancestor 1. "always" is load-bearing: without it this needle also
    # matches the CORRECT tip-CHANGED arm and the test goes permanently red.
    "always take the tip's version verbatim":
        "unconditional take-the-tip (66eb3ea5..09e30f75) -- the wording that "
        "reset a verified regen",
    # Ancestor 2, both halves of its gate.
    "ONLY IF THE MERGE CONFLICTS ON IT":
        "conflict-conditional gate (6b31f38e..5c3df01f^) -- gates on conflict, "
        "not on content, so a silent auto-merge chimera is taken",
    "If the merge does not touch it, LEAVE IT ALONE":
        "conflict-conditional companion clause (6b31f38e..5c3df01f^)",
}


class RenderedPromptsTest(ClauseAssertions):
    """Both prompts render; the f-strings actually interpolate."""

    def setUp(self):
        super().setUp()
        self.converge = agentworker.build_prompt(BRANCH, HUB_URL, TIP_SHA, HOST)
        self.authoring = agentworker.build_authoring_prompt(
            SLUG, INTENT, HUB_URL, TIP_SHA, HOST)

    def test_both_prompts_render_nonempty(self):
        self.assertGreater(len(self.converge), 500)
        self.assertGreater(len(self.authoring), 500)

    def test_placeholders_interpolate(self):
        """A dropped `f` prefix turns the whole doctrine into literal braces
        and every present-check below would still pass on the source text.
        Pin the interpolation itself."""
        for label, prompt, placeholders, expect in (
            ("build_prompt", self.converge,
             ("{tip_sha}", "{branch}", "{host}", "{hub_url}"),
             (TIP_SHA, BRANCH, HOST, HUB_URL)),
            # NOTE: build_authoring_prompt accepts tip_sha but never renders
            # it -- see test_authoring_prompt_does_not_use_tip_sha below.
            ("build_authoring_prompt", self.authoring,
             ("{tip_sha}", "{slug}", "{host}"),
             (SLUG, HOST)),
        ):
            for ph in placeholders:
                self.assertNotIn(ph, prompt, f"{label}: {ph} survived un-rendered")
            for want in expect:
                self.assertIn(want, prompt, f"{label}: {want!r} never interpolated")

    def test_authoring_prompt_does_not_use_tip_sha(self):
        """FINDING, pinned as current behaviour rather than asserted as
        correct: `build_authoring_prompt(slug, intent, hub_url, tip_sha, host)`
        takes `tip_sha` and never interpolates it -- and `hub_url` likewise.
        The prompt only claims the clone is "checked out at the integration
        tip"; it never names WHICH sha that is, so an agent cannot verify the
        claim and neither can a reader of its output.

        Harmless today (agentworker.run does check out `tip_sha`), but it is
        the "name the instrument" gap in prompt form. If the parameter is ever
        wired in, this test goes red -- that is the signal to move the sha
        into the present-checks, not to delete the assertion."""
        self.assertNoClause(self.authoring, TIP_SHA)
        self.assertNoClause(self.authoring, HUB_URL)

    def test_authoring_prompt_survives_a_scopeless_intent(self):
        """`intent.get("scope") or {}` -- the crashfix in 5c3df01f. An intent
        with no scope, or a null one, must still render."""
        for intent in ({"title": "t"}, {"title": "t", "scope": None}, {}):
            p = agentworker.build_authoring_prompt(SLUG, intent, HUB_URL, TIP_SHA, HOST)
            self.assertClause(p, "(see title)")


class ContentConditionalRulePresentTest(ClauseAssertions):
    """ASSERT PRESENT: the corrected, content-conditional generated-file rule
    (5c3df01f). Each needle is one clause the defect deleted."""

    def setUp(self):
        super().setUp()
        self.prompt = agentworker.build_prompt(BRANCH, HUB_URL, TIP_SHA, HOST)

    def test_captures_base_and_branch_before(self):
        """Both shas the content check needs, captured BEFORE the merge --
        neither is recoverable afterwards."""
        self.assertClause(self.prompt, f"BASE=$(git merge-base HEAD {TIP_SHA})")
        self.assertClause(self.prompt, "BRANCH_BEFORE=$(git rev-parse HEAD)")
        self.assertClause(self.prompt, "BEFORE merging")

    def test_decides_by_content_not_by_conflict(self):
        """The merge-base diff IS the rule. Without it there is no way to
        tell a branch's own regen from a stale copy."""
        self.assertClause(self.prompt, "decided by CONTENT")
        self.assertClause(self.prompt, f"git diff --quiet $BASE {TIP_SHA} -- {GENERATED}")

    def test_tip_unchanged_arm_leaves_the_branch_copy_alone(self):
        """The arm whose absence caused the corruption."""
        self.assertClause(
            self.prompt,
            "Tip UNCHANGED since the base (diff empty): LEAVE THE FILE ALONE")
        self.assertClause(self.prompt, "silently destroys completed regen work")
        # verified by content against the branch, not against the tip
        self.assertClause(self.prompt, f"git diff $BRANCH_BEFORE -- {GENERATED}")
        # and the agent is told the tip-diff being non-empty is CORRECT here,
        # so it does not "helpfully" reconcile it back to the tip
        self.assertClause(self.prompt, "is EXPECTED and CORRECT here")

    def test_tip_changed_arm_carries_the_chimera_clause(self):
        """The half the conflict-conditional ancestor was missing: a clean
        textual auto-merge of two regens is still garbage."""
        self.assertClause(self.prompt, "Tip CHANGED since the base (diff non-empty)")
        self.assertClause(self.prompt, "even if git auto-merged the file without conflicting")
        self.assertClause(
            self.prompt,
            "a textual auto-merge of two regens is a chimera no generator ever produced")
        # this arm verifies by content against the TIP
        self.assertClause(self.prompt, f"git diff {TIP_SHA} -- {GENERATED}")

    def test_never_hand_edit_survives_on_both_prompts(self):
        authoring = agentworker.build_authoring_prompt(
            SLUG, INTENT, HUB_URL, TIP_SHA, HOST)
        self.assertClause(self.prompt, "never hand-edit it")
        self.assertClause(authoring, f"Never edit {GENERATED} by hand (generated)")


class DefectiveAncestorsAbsentTest(ClauseAssertions):
    """ASSERT ABSENT: neither defective ancestor's wording is back.

    This is the half that catches a regression. A present-only test passes
    happily when the old rule is APPENDED next to the new one -- which is
    the likeliest way it comes back."""

    def setUp(self):
        super().setUp()
        self.prompts = {
            "build_prompt": agentworker.build_prompt(BRANCH, HUB_URL, TIP_SHA, HOST),
            "build_authoring_prompt": agentworker.build_authoring_prompt(
                SLUG, INTENT, HUB_URL, TIP_SHA, HOST),
        }

    def test_no_defective_ancestor_wording_in_any_prompt(self):
        # Hand-rolled rather than assertNotIn: assertNotIn dumps the whole
        # multi-KB prompt ahead of the message, which buries the one line a
        # reader needs. Fail with the diagnosis first.
        for label, prompt in self.prompts.items():
            for needle, why in FORBIDDEN.items():
                if needle in prompt:
                    self.fail(
                        f"{label} reintroduced defective doctrine.\n"
                        f"  found:  {needle!r}\n"
                        f"  origin: {why}\n"
                        f"  fix:    the rule must stay CONTENT-conditional -- "
                        f"decide by `git diff $BASE <tip> -- {GENERATED}`, "
                        f"not by whether git reported a conflict.\n"
                        f"  context: ...{_around(prompt, needle)}...")

    def test_forbidden_needles_match_the_real_defect_specimens(self):
        """A needle that no longer matches its own defect is decoration.

        Prove each one still fires on the historical text it was cut from,
        so the absent-checks above are known-discriminating rather than
        merely known-passing."""
        specimens = {
            "66eb3ea5..09e30f75 (unconditional)": DEFECT_UNCONDITIONAL,
            "6b31f38e..5c3df01f^ (conflict-conditional)": DEFECT_CONFLICT_CONDITIONAL,
        }
        for label, specimen in specimens.items():
            hits = [n for n in FORBIDDEN if n in specimen]
            self.assertTrue(
                hits, f"no forbidden needle matches the {label} specimen; the "
                      f"absent-checks can no longer catch it")

    def test_correct_prompt_keeps_the_conditional_take_tip_phrase(self):
        """Guards the trap named in the module docstring: the correct rule
        DOES say "take the tip's version verbatim" on the tip-CHANGED arm.
        If someone tightens the forbidden needle by dropping "always", this
        test explains why the suite then goes red for the wrong reason."""
        prompt = self.prompts["build_prompt"]
        self.assertClause(prompt, "take the tip's version verbatim")
        self.assertNoClause(prompt, "always take the tip's version verbatim")


class GuardrailsPresentTest(ClauseAssertions):
    """The other hard rules the prompts are the only carrier of."""

    def setUp(self):
        super().setUp()
        self.prompts = {
            "build_prompt": agentworker.build_prompt(BRANCH, HUB_URL, TIP_SHA, HOST),
            "build_authoring_prompt": agentworker.build_authoring_prompt(
                SLUG, INTENT, HUB_URL, TIP_SHA, HOST),
        }

    def test_never_push_to_main_or_the_integration_tip(self):
        for label, prompt in self.prompts.items():
            self.assertClause(prompt, "Never push to `main`", label)
            self.assertClause(prompt, "refactor/tag-machinery", label)

    def test_pinned_exiftool_only(self):
        """AGENTS.md: a bare `exiftool` resolves to whatever PATH has and
        manufactures phantom regressions AND phantom fixes."""
        for label, prompt in self.prompts.items():
            self.assertClause(prompt, "Never invoke bare `exiftool`", label)
            self.assertClause(prompt, "/tmp/oxidex-exiftool-cache/exiftool-pinned.sh", label)

    def test_no_weakening_tests_to_get_green(self):
        self.assertClause(self.prompts["build_prompt"],
                          "Do not weaken any test or gate to get green")
        self.assertClause(self.prompts["build_authoring_prompt"],
                          "Do not weaken any existing test")

    def test_authoring_prompt_carries_the_omit_rather_than_approximate_law(self):
        """AGENTS.md: a plausible-but-wrong value under a real tag name is
        worse than an absent tag."""
        p = self.prompts["build_authoring_prompt"]
        self.assertClause(p, "NEVER approximate a conversion")
        self.assertClause(p, "absence is correct output")

    def test_authoring_prompt_names_the_instrument_for_its_own_claim(self):
        """AGENTS.md: state the instrument alongside the number."""
        p = self.prompts["build_authoring_prompt"]
        self.assertClause(p, "under scripts/compare_file.py")
        self.assertClause(p, f"git push origin HEAD:refs/heads/staging/{SLUG}")


if __name__ == "__main__":
    unittest.main()
