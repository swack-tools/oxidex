"""Tests for the canonical Claude-to-Codex skill mirror."""

from __future__ import annotations

import pathlib
import shutil
import tempfile
import unittest

from tools.ci import sync_agent_skills as sync


REPO = pathlib.Path(__file__).resolve().parents[2]


def canonical(skill: str, relative: str) -> str:
    """Read a file from the canonical shared-skill tree."""

    return (REPO / ".claude/skills" / skill / relative).read_text(encoding="utf-8")


def make_fixture(root: pathlib.Path, *, canonical: str, mirror: str) -> pathlib.Path:
    """Create the smallest repository fixture needed by the mirror tests."""

    (root / ".claude/skills/alpha").mkdir(parents=True)
    (root / ".agents/skills/alpha").mkdir(parents=True)
    (root / ".gitignore").write_text(
        ".claude/*\n"
        "!.claude/skills/\n"
        ".claude/skills/*\n"
        "!.claude/skills/alpha/\n",
        encoding="utf-8",
    )
    (root / ".claude/skills/alpha/SKILL.md").write_text(canonical, encoding="utf-8")
    (root / ".agents/skills/alpha/SKILL.md").write_text(mirror, encoding="utf-8")
    return root


class SkillMirrorTests(unittest.TestCase):
    def test_shared_skill_allowlist_is_discovered(self):
        self.assertIn("exiftool-parity", sync.shared_skill_names(REPO))

    def test_agents_mirror_matches_canonical(self):
        self.assertEqual(sync.compare_skill_mirror(REPO), [])

    def test_release_finalization_contract(self):
        text = canonical("oxidex-release-finalization", "SKILL.md")
        for phrase in (
            "reviewed PR",
            "exact `main` commit",
            "explicit maintainer",
            "Gatekeeper",
            "stapled",
            "do not move",
        ):
            self.assertIn(phrase, text)

    def test_check_mode_reports_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_fixture(pathlib.Path(tmp), canonical="alpha", mirror="beta")
            self.assertNotEqual(sync.main(["--repo", str(repo), "--check"]), 0)

    def test_missing_canonical_skill_is_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_fixture(pathlib.Path(tmp), canonical="alpha", mirror="alpha")
            shutil.rmtree(repo / ".claude/skills/alpha")
            differences = sync.compare_skill_mirror(repo)
            self.assertTrue(any("canonical skill" in difference for difference in differences))
            self.assertNotEqual(sync.main(["--repo", str(repo), "--check"]), 0)

    def test_write_replaces_only_allowlisted_skill(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_fixture(pathlib.Path(tmp), canonical="alpha", mirror="stale")
            mirror_skill = repo / ".agents/skills/alpha"
            (mirror_skill / "stale.txt").write_text("stale", encoding="utf-8")
            unlisted = repo / ".agents/skills/unlisted"
            unlisted.mkdir()
            sentinel = unlisted / "SENTINEL"
            sentinel.write_text("keep", encoding="utf-8")

            sync.write_skill_mirror(repo)

            self.assertTrue((repo / ".agents").is_dir())
            self.assertTrue((repo / ".agents/skills").is_dir())
            self.assertTrue(sentinel.is_file())
            self.assertEqual(sync.compare_skill_mirror(repo), [])
            self.assertFalse((mirror_skill / "stale.txt").exists())


if __name__ == "__main__":
    unittest.main()
