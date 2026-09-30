"""Fail-closed contract tests for the main CI workflow entry points.

The main ruleset must require these CI contexts for both pull requests and
merge-queue candidates. A post-merge push still supplies separate evidence
for the exact resulting main SHA; neither a PR check nor a merge-group check
is evidence that the post-merge run completed successfully.

These tests deliberately inspect the committed workflow text instead of
depending on PyYAML, because the lint job runs on bare ``python3``.
"""
import json
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest


CI_YAML = pathlib.Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
RUNNER_ACTION = (
    pathlib.Path(__file__).resolve().parents[2]
    / ".github/actions/select-m4air-runner/action.yml"
)


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

    def test_workflow_uses_least_privilege_read_only_token(self):
        self.assertRegex(self.text, r"(?m)^permissions:\n  contents: read$")


class RunnerSelectorTests(unittest.TestCase):
    """Exercise the committed action, including its org API calls and jq."""

    @staticmethod
    def runner(labels, *, status="online", busy=False):
        return {
            "status": status,
            "busy": busy,
            "labels": [{"name": label} for label in labels],
        }

    @staticmethod
    def action_script():
        lines = RUNNER_ACTION.read_text().splitlines()
        start = lines.index("      run: |") + 1
        body = []
        for line in lines[start:]:
            if line and not line.startswith("        "):
                break
            body.append(line[8:])
        return "\n".join(body) + "\n"

    def test_org_runner_eligibility_and_fallbacks(self):
        self.assertIsNotNone(shutil.which("jq"), "CI selector requires real jq")
        selected = 'runs_on=["self-hosted","Linux","ARM64","oxidex-m4air"]'
        fallback = 'runs_on="ubuntu-latest"'
        labels = ["self-hosted", "Linux", "ARM64", "oxidex-m4air"]
        r = self.runner
        # Each response is a list of pages, as returned by gh --paginate --slurp.
        cases = {
            "missing-token": ([[]], [[{"id": 17, "name": "oxidex-m4air"}]], False, "", fallback),
            "group-api-failure": ([[]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "groups", fallback),
            "missing-group": ([[]], [[{"id": 12, "name": "other"}]], True, "", fallback),
            "runners-api-failure": ([[]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "runners", fallback),
            "empty": ([[]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "idle-correct": ([[r(labels)]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", selected),
            "busy-correct": ([[r(labels, busy=True)]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "offline-correct": ([[r(labels, status="offline")]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "wrong-os": ([[r(["self-hosted", "macOS", "ARM64", "oxidex-m4air"])]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "wrong-arch": ([[r(["self-hosted", "Linux", "X64", "oxidex-m4air"])]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "missing-self-hosted": ([[r(labels[1:])]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "distributed-labels": ([[r(labels[:3]), r(["oxidex-m4air"])]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "one-idle-one-busy": ([[r(labels, busy=True), r(labels)]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", selected),
            "busy-correct-idle-wrong-os": ([[r(labels, busy=True), r(["self-hosted", "macOS", "ARM64", "oxidex-m4air"])]], [[{"id": 17, "name": "oxidex-m4air"}]], True, "", fallback),
            "paginated-group-and-runners": ([[r(labels, busy=True)], [r(labels)]], [[{"id": 12, "name": "other"}], [{"id": 17, "name": "oxidex-m4air"}]], True, "", selected),
        }
        groups_endpoint = "orgs/swack-tools/actions/runner-groups?per_page=100"
        runners_endpoint = "orgs/swack-tools/actions/runner-groups/17/runners?per_page=100"
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            script = root / "selector.sh"
            script.write_text(self.action_script())
            mock_bin = root / "bin"
            mock_bin.mkdir()
            gh = mock_bin / "gh"
            gh.write_text(
                '#!/bin/sh\nurl=""\nfor arg in "$@"; do url="$arg"; done\n'
                'printf "%s\\n" "$url" >> "$MOCK_API_LOG"\n'
                'case "$url" in\n'
                f'  "{groups_endpoint}") [ "$MOCK_FAIL" = groups ] && exit 1; cat "$MOCK_GROUPS" ;;\n'
                f'  "{runners_endpoint}") [ "$MOCK_FAIL" = runners ] && exit 1; cat "$MOCK_RUNNERS" ;;\n'
                '  *) exit 90 ;;\nesac\n'
            )
            gh.chmod(0o755)
            for name, (runner_pages, group_pages, token, failure, expected) in cases.items():
                with self.subTest(name=name):
                    case = root / name
                    case.mkdir()
                    (case / "groups.json").write_text(json.dumps(
                        [{"runner_groups": page} for page in group_pages]
                    ))
                    (case / "runners.json").write_text(json.dumps(
                        [{"runners": page} for page in runner_pages]
                    ))
                    env = dict(os.environ)
                    env.update({
                        "PATH": f"{mock_bin}{os.pathsep}{os.environ['PATH']}",
                        "GH_TOKEN": "mock-token" if token else "",
                        "GITHUB_REPOSITORY": "swack-tools/oxidex",
                        "GITHUB_OUTPUT": str(case / "output"),
                        "MOCK_API_LOG": str(case / "api.log"),
                        "MOCK_GROUPS": str(case / "groups.json"),
                        "MOCK_RUNNERS": str(case / "runners.json"),
                        "MOCK_FAIL": failure,
                    })
                    run = subprocess.run(
                        ["bash", str(script)], env=env, capture_output=True, text=True,
                        timeout=10,
                    )
                    self.assertEqual(run.returncode, 0, run.stderr)
                    self.assertEqual((case / "output").read_text().strip(), expected)
                    calls = (case / "api.log").read_text().splitlines() if (case / "api.log").exists() else []
                    wanted = [] if not token else [groups_endpoint]
                    if token and failure != "groups" and name != "missing-group":
                        wanted.append(runners_endpoint)
                    self.assertEqual(calls, wanted)


if __name__ == "__main__":
    unittest.main()
