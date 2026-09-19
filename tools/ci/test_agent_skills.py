"""Tests for the canonical Claude-to-Codex skill mirror."""

from __future__ import annotations

import json
import pathlib
import re
import shutil
import tempfile
import unittest

from tools.ci import sync_agent_skills as sync


REPO = pathlib.Path(__file__).resolve().parents[2]


def canonical(skill: str, relative: str) -> str:
    """Read a file from the canonical shared-skill tree."""

    return (REPO / ".claude/skills" / skill / relative).read_text(encoding="utf-8")


def bash_snippets(skill: str, relative: str) -> list[str]:
    """Return fenced Bash snippets from one canonical skill file."""

    return re.findall(r"```bash\n(.*?)```", canonical(skill, relative), re.DOTALL)


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
    def test_parity_markdown_has_no_bare_oracle_command(self):
        skill = REPO / ".claude/skills/exiftool-parity"
        # Command tokens, including inline examples, must use the pinned argv.
        bare = re.compile(r"(?<![\w/.-])exiftool\s+(?:-[A-Za-z]|FILE\b)")
        for path in skill.rglob("*.md"):
            with self.subTest(path=path.relative_to(skill)):
                self.assertNotRegex(path.read_text(encoding="utf-8"), bare)

    def test_parity_release_instrument_contract(self):
        text = canonical("exiftool-parity", "SKILL.md")
        for phrase in (
            "/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2",
            ".exiftool-version", "DOCX", "--recursive", "--min-files",
            "--min-tags", "--json-out", "blocked", "strict.pm",
        ):
            self.assertIn(phrase, text)

    def test_parity_receipt_separates_measurement_families(self):
        path = REPO / ".claude/skills/exiftool-parity/templates/release-parity-receipt.json"
        self.assertTrue(path.is_file(), "release parity receipt is absent")
        receipt = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(receipt["schema_version"], 1)
        self.assertEqual(receipt["status"], "unverified")
        for field in ("oxidex_sha", "exiftool_version", "oracle", "corpora", "regressions", "refusals"):
            self.assertIn(field, receipt)
        for family in ("conformance", "authenticated_reads", "generated_catalog", "write_matrix"):
            self.assertIsInstance(receipt[family], dict)
            self.assertEqual(receipt[family]["status"], "unverified")
        self.assertNotIn("overall_parity_percent", receipt)

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

    def test_release_documentation_contract(self):
        skill = REPO / ".claude/skills/oxidex-release-documentation"
        self.assertTrue((skill / "SKILL.md").is_file(), "documentation skill is absent")
        text = "\n".join(
            canonical("oxidex-release-documentation", relative)
            for relative in (
                "SKILL.md",
                "references/factuality-ledger.md",
                "references/github-pages-audit.md",
                "references/benchmark-policy.md",
            )
        )
        for phrase in (
            "every rendered route", "current", "historical", "mobile", "dark theme",
            "build_type", "workflow", "gh-pages", "exact candidate commit",
            "tools/docs-local-deploy.sh", "live deployment",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)

    def test_release_documentation_receipt_contract(self):
        path = REPO / ".claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.json"
        self.assertTrue(path.is_file(), "documentation receipt is absent")
        receipt = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(receipt["schema_version"], 1)
        for field in (
            "version", "candidate_sha", "parity_receipt", "claims", "pages", "benchmarks",
            "local_build", "visual_review", "pages_pipeline", "live_deployment", "status", "unresolved",
        ):
            self.assertIn(field, receipt)
        self.assertIsInstance(receipt["pages"], list)
        self.assertIsInstance(receipt["claims"], list)
        self.assertEqual(receipt["status"], "unverified")
        self.assertEqual(receipt["live_deployment"]["status"], "not_run")

    def test_release_documentation_verifies_local_candidate_without_deployment(self):
        receipt = json.loads(canonical(
            "oxidex-release-documentation", "templates/documentation-release-receipt.json"
        ))
        self.assertIs(receipt["live_deployment"].get("required_for_documentation_verification"), False)
        self.assertNotIn("promotion_readiness", receipt)
        self.assertNotIn("phase", receipt)
        self.assertIsNone(receipt["candidate_tree"])
        self.assertEqual(receipt["visual_review"]["human_review"]["status"], "unverified")
        self.assertIn("automation_manifest", receipt["visual_review"])
        skill = canonical("oxidex-release-documentation", "SKILL.md")
        for phrase in ("status: verified", "optional", "before tag authorization", "human screenshot review"):
            self.assertIn(phrase, skill)
        audit = canonical("oxidex-release-documentation", "references/github-pages-audit.md")
        for phrase in ("Playwright", "requestfailed", "pageerror", "1440", "390", "actionlint"):
            self.assertIn(phrase, audit)

    def test_release_finalization_pipeline_snippets_enable_pipefail(self):
        for relative in ("references/gates.md", "references/github-release-and-macos.md"):
            for index, snippet in enumerate(bash_snippets("oxidex-release-finalization", relative)):
                if "|" not in snippet:
                    continue
                first_command = next(line.strip() for line in snippet.splitlines() if line.strip())
                with self.subTest(relative=relative, snippet=index):
                    self.assertEqual(first_command, "set -euo pipefail")

    def test_release_finalization_reruns_are_bound_to_main_sha(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        for phrase in (
            "POST_MERGE_WORKTREE",
            'git worktree add --detach "$POST_MERGE_WORKTREE" "$MAIN_SHA"',
            'MAIN_HEAD=$(git -C "$POST_MERGE_WORKTREE" rev-parse \'HEAD^{commit}\')',
            'test "$MAIN_HEAD" = "$MAIN_SHA"',
            "MAIN_CARGO_TARGET_DIR",
            "MAIN_EVIDENCE_DIR",
        ):
            self.assertIn(phrase, text)

    def test_release_finalization_selects_tag_bound_workflow_runs(self):
        text = canonical(
            "oxidex-release-finalization", "references/github-release-and-macos.md"
        )
        for phrase in (
            "headBranch",
            ".headBranch == $tag",
            ".headSha == $sha",
            ".workflowName == $workflow",
            '.event == "push"',
            "length == 1",
            "selected-release-run.json",
            "selected-docker-run.json",
            "RELEASE_RUN_ID=$(jq -er",
            "DOCKER_RUN_ID=$(jq -er",
        ):
            self.assertIn(phrase, text)

    def test_release_finalization_persists_successful_final_workflow_runs(self):
        text = canonical(
            "oxidex-release-finalization", "references/github-release-and-macos.md"
        )
        for run_id, workflow, evidence, next_marker in (
            (
                "RELEASE_RUN_ID",
                "Release",
                "final-release-run.json",
                'gh run watch "$DOCKER_RUN_ID"',
            ),
            ("DOCKER_RUN_ID", "Docker", "final-docker-run.json", "```"),
        ):
            watch = f'gh run watch "${run_id}" --exit-status'
            view = f'gh run view "${run_id}" --json'
            with self.subTest(workflow=workflow):
                self.assertIn(view, text)
                self.assertIn(f'> "$EVIDENCE_DIR/{evidence}"', text)
                self.assertLess(text.index(watch), text.index(view))
                view_index = text.index(view)
                final_record = text[view_index : text.index(next_marker, view_index)]
                for phrase in (
                    ".status == \"completed\"",
                    ".conclusion == \"success\"",
                    ".headBranch == $tag",
                    ".headSha == $sha",
                    ".workflowName == $workflow",
                    ".event == \"push\"",
                    f'--arg workflow "{workflow}"',
                    f'"$EVIDENCE_DIR/{evidence}"',
                ):
                    self.assertIn(phrase, final_record)

    def test_release_finalization_captures_created_pr(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        for phrase in (
            "PR_URL=$(gh pr create",
            'printf \'%s\\n\' "$PR_URL"',
            'PR=$(gh pr view "$PR_URL"',
            'gh pr view "$PR"',
            'gh pr checks "$PR"',
        ):
            self.assertIn(phrase, text)

    def test_release_finalization_uses_durable_evidence_paths(self):
        gates = canonical("oxidex-release-finalization", "references/gates.md")
        github = canonical(
            "oxidex-release-finalization", "references/github-release-and-macos.md"
        )
        entrypoint = canonical("oxidex-release-finalization", "SKILL.md")
        self.assertIn("EVIDENCE_ROOT=/absolute/durable/evidence/root", gates)
        self.assertIn("outside tracked repository content", entrypoint)
        self.assertNotIn("/tmp/oxidex-release", gates + github)
        self.assertNotRegex(gates + github, r"(?m)^\s*rm\s+-[^\n]*r")

    def test_release_finalization_verifies_ssh_signed_tag(self):
        text = canonical("oxidex-release-finalization", "references/gates.md")
        for phrase in (
            "gpg.ssh.allowedSignersFile",
            "user.signingkey",
            'tag -v "$TAG"',
            '"${VERIFY[@]}"',
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
