"""Tests for the canonical Claude-to-Codex skill mirror."""

from __future__ import annotations

import pathlib
import tempfile
import unittest

from tools.ci import sync_agent_skills as sync


REPO = pathlib.Path(__file__).resolve().parents[2]


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

    def test_check_mode_reports_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_fixture(pathlib.Path(tmp), canonical="alpha", mirror="beta")
            self.assertNotEqual(sync.main(["--repo", str(repo), "--check"]), 0)


if __name__ == "__main__":
    unittest.main()
