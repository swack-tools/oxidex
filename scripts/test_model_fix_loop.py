import json
import unittest
from unittest.mock import patch, MagicMock

from model_fix_loop import extract_diff, call_model


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


if __name__ == "__main__":
    unittest.main()
