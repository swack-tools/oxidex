import json
import unittest
from unittest.mock import patch, MagicMock
from pathlib import Path

from model_fix_loop import (
    cargo_build,
    cargo_test_workspace,
    call_model,
    extract_diff,
    git_apply,
    git_checkout_clean,
    git_commit,
)


class ExtractDiffTests(unittest.TestCase):
    def test_extracts_fenced_diff_block(self):
        text = (
            "Here is the fix:\n```diff\n--- a/foo.rs\n+++ b/foo.rs\n"
            "@@ -1 +1 @@\n-old\n+new\n```\nDone."
        )
        diff = extract_diff(text)
        self.assertTrue(diff.startswith("--- a/foo.rs"))
        self.assertIn("+new", diff)

    def test_falls_back_to_bare_diff_git_header(self):
        text = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1 +1 @@\n-old\n+new\n"
        self.assertEqual(extract_diff(text), text)

    def test_returns_none_when_no_diff_present(self):
        self.assertIsNone(extract_diff("I don't know how to fix this."))


class CallModelTests(unittest.TestCase):
    @patch("model_fix_loop.urllib.request.urlopen")
    def test_posts_expected_body_and_parses_reply(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        mock_cm = MagicMock()
        mock_cm.read.return_value = response_json
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        result = call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
        )

        self.assertEqual(result, "the diff")
        request = mock_urlopen.call_args[0][0]
        self.assertEqual(request.full_url, "https://api.z.ai/api/paas/v4/chat/completions")
        self.assertEqual(request.get_header("Authorization"), "Bearer secret")
        body = json.loads(request.data)
        self.assertEqual(body["model"], "glm-5.2")
        self.assertEqual(body["messages"], [{"role": "user", "content": "fix it"}])


class GitApplyTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_success_returns_true(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stderr="")
        ok, msg = git_apply("diff text", Path("/fake/repo"))
        self.assertTrue(ok)
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["git", "apply", "--reject", "-"])
        self.assertEqual(kwargs["input"], "diff text")
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))

    @patch("model_fix_loop.subprocess.run")
    def test_failure_returns_stderr(self, mock_run):
        mock_run.return_value = MagicMock(returncode=1, stderr="patch does not apply")
        ok, msg = git_apply("bad diff", Path("/fake/repo"))
        self.assertFalse(ok)
        self.assertEqual(msg, "patch does not apply")


class GitCheckoutCleanTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_runs_checkout_then_clean(self, mock_run):
        git_checkout_clean(Path("/fake/repo"))
        calls = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(["git", "checkout", "--", "."], calls)
        self.assertIn(["git", "clean", "-fd"], calls)


class GitCommitTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_adds_then_commits_with_message(self, mock_run):
        git_commit("fix(nef): wire tags", Path("/fake/repo"))
        calls = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(["git", "add", "-A"], calls)
        self.assertIn(["git", "commit", "-m", "fix(nef): wire tags"], calls)


class CargoBuildTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_reports_failure_with_stderr(self, mock_run):
        mock_run.return_value = MagicMock(returncode=101, stderr="error[E0308]: mismatched types")
        ok, err = cargo_build(Path("/fake/repo"))
        self.assertFalse(ok)
        self.assertIn("E0308", err)

    @patch("model_fix_loop.subprocess.run")
    def test_reports_success(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stderr="")
        ok, err = cargo_build(Path("/fake/repo"))
        self.assertTrue(ok)


class CargoTestWorkspaceTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_true_on_zero_exit(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        self.assertTrue(cargo_test_workspace(Path("/fake/repo")))

    @patch("model_fix_loop.subprocess.run")
    def test_false_on_nonzero_exit(self, mock_run):
        mock_run.return_value = MagicMock(returncode=1)
        self.assertFalse(cargo_test_workspace(Path("/fake/repo")))


if __name__ == "__main__":
    unittest.main()
