"""Fail-closed contract tests for the main CI workflow entry points.

The main ruleset must require these CI contexts for both pull requests and
merge-queue candidates. A post-merge push still supplies separate evidence
for the exact resulting main SHA; neither a PR check nor a merge-group check
is evidence that the post-merge run completed successfully.

These tests deliberately inspect the committed workflow text instead of
depending on PyYAML, because the lint job runs on bare ``python3``.
"""
import pathlib
import re
import unittest


CI_YAML = pathlib.Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"


def trigger_block(text: str) -> str:
    """Return the top-level ``on:`` mapping, refusing a malformed layout."""
    match = re.search(r"(?ms)^on:\n(.*?)(?=^[A-Za-z][A-Za-z0-9_-]*:|\Z)", text)
    if match is None:
        raise AssertionError("top-level on: mapping not found")
    return match.group(1)


class MainCiWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = CI_YAML.read_text()
        cls.triggers = trigger_block(cls.text)

    def test_main_push_and_pull_request_routes_are_preserved(self):
        self.assertRegex(
            self.triggers,
            r"(?m)^  push:\n    branches: \[main, refactor/tag-machinery\]$",
        )
        self.assertRegex(self.triggers, r"(?m)^  pull_request:\n    branches: \['\*\*'\]$")

    def test_main_merge_queue_candidates_run_ci(self):
        self.assertRegex(self.triggers, r"(?m)^  merge_group:\n    branches: \[main\]$")

    def test_manual_route_is_preserved(self):
        self.assertRegex(self.triggers, r"(?m)^  workflow_dispatch:$")

    def test_direct_architecture_routing_needs_no_runner_credential(self):
        def block(job):
            return re.search(r"(?ms)^  " + re.escape(job) + r":\n(.*?)(?=^  [a-zA-Z0-9_-]+:|\Z)", self.text).group(1)
        for job in ("read-regression-gate", "verify-tables-capture"):
            with self.subTest(job=job):
                self.assertIn("runs-on: [self-hosted, Linux, ARM64]", block(job))
        self.assertIn("needs: [read-regression-gate]", block("verify-tables-capture"))
        self.assertIn("if: ${{ always() && !cancelled() }}", block("verify-tables-capture"))
        for obsolete in ("RUNNER_STATUS_TOKEN", "select_read_gate_runner", "select_capture_runner", "select-m4air-runner"):
            self.assertNotIn(obsolete, self.text)
        for job in ("lint", "test", "release-build"):
            with self.subTest(job=job):
                self.assertIn("runs-on: spot", block(job))

    def test_no_workflow_or_lint_config_routes_to_warpbuild(self):
        github = CI_YAML.parents[1]
        for path in sorted([github / "actionlint.yaml", *github.glob("workflows/*.yml"), *github.glob("workflows/*.yaml")]):
            with self.subTest(path=path.name):
                self.assertNotRegex(path.read_text(), r"(?i)warp")

    def test_workflow_uses_least_privilege_read_only_token(self):
        self.assertRegex(self.text, r"(?m)^permissions:\n  contents: read$")


if __name__ == "__main__":
    unittest.main()
