"""`tools/exiftool-tables/fixtures/` must hold only what CI can account for.

CI's "Verify Generated Tables / staleness and drift" job regenerates that
directory from the pinned ExifTool dump with `gen_staleness_facts.py` and
requires `diff -ru` equality against the committed copy, excluding a short list
of pin-derived fixtures that are checked some other way. Any other file makes
the diff report "Only in tools/exiftool-tables/fixtures: <name>" and the job
fail.

Two PRs in one session (#802's Garmin 11.78 fact, #810's Sony model
conditions) committed historical-release test data there and found out only
about ten minutes into `verify-tables`, after the full native capture. Those
files belong in `tools/exiftool-tables/testdata/`, which already holds
`raw_jfif_11_78_fact.json` and friends.

This is the same check, made static so it fails in the lint job in under a
second, with the fix in the message. It reads both sides from their sources
rather than restating them: the names `gen_staleness_facts.py` writes, and the
`--exclude` list in `ci.yml`. So it cannot drift from either.
"""
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "tools" / "exiftool-tables" / "fixtures"
GENERATOR = ROOT / "tools" / "exiftool-tables" / "gen_staleness_facts.py"
CI_YAML = ROOT / ".github" / "workflows" / "ci.yml"


def generated_names():
    """Every fixture file name `gen_staleness_facts.py` writes, read from its source.

    The script names each output as a string literal. A name assembled at run
    time would not appear here, so this fails CLOSED: such a file would be
    reported as unaccounted for rather than silently waved through.
    """
    return set(re.findall(r'"([A-Za-z0-9_.-]+\.json)"', GENERATOR.read_text()))


def excluded_names():
    """The `--exclude` list on the staleness job's `diff -ru` in ci.yml."""
    text = CI_YAML.read_text()
    anchor = text.find("diff -ru tools/exiftool-tables/fixtures")
    if anchor < 0:
        return None
    # The diff invocation is one shell command continued with backslashes and
    # closed by `; then` -- read exactly that span, no further.
    end = text.find("; then", anchor)
    return set(re.findall(r"--exclude\s+(\S+)", text[anchor:end if end > 0 else None]))


class FixturesDirectoryTests(unittest.TestCase):
    def test_the_staleness_diff_is_still_where_this_test_reads_it(self):
        self.assertIsNotNone(
            excluded_names(),
            "ci.yml no longer contains `diff -ru tools/exiftool-tables/fixtures`; "
            "update this test alongside the staleness job",
        )

    def test_every_fixture_is_generated_or_explicitly_excluded(self):
        excluded = excluded_names() or set()
        accounted = generated_names() | excluded
        present = {path.name for path in FIXTURES.iterdir() if path.is_file()}
        stray = sorted(present - accounted)
        self.assertEqual(
            stray,
            [],
            "these files in tools/exiftool-tables/fixtures/ are neither written by "
            "gen_staleness_facts.py nor excluded from CI's staleness diff, so the "
            "'staleness and drift' job will fail on them. Historical-release or "
            "hand-captured test data belongs in tools/exiftool-tables/testdata/. "
            "Only add to ci.yml's --exclude list for a pin-derived fixture that is "
            f"verified some other way: {stray}",
        )

    def test_every_exclusion_still_names_a_real_fixture(self):
        # A stale exclusion is harmless to the diff but hides that a fixture
        # was deleted or renamed; keep the list honest.
        present = {path.name for path in FIXTURES.iterdir() if path.is_file()}
        self.assertEqual(sorted((excluded_names() or set()) - present), [])

    def test_generator_names_are_read_from_its_source(self):
        # Guards the premise: if this ever reads nothing, every fixture would
        # look stray and the message above would mislead.
        self.assertGreaterEqual(len(generated_names()), 1)


if __name__ == "__main__":
    unittest.main()
