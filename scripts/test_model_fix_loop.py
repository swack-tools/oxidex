import json
import os
import tempfile
import unittest
from unittest.mock import patch, MagicMock
from pathlib import Path

from model_fix_loop import (
    _load_dotenv,
    attempt_build,
    build_prompt,
    build_review_prompt,
    cargo_build,
    cargo_test_workspace,
    call_model,
    extract_diff,
    extract_review_verdict,
    fix_gap,
    git_apply,
    git_checkout_clean,
    git_commit,
    review_verdict,
    run_loop,
)


class LoadDotenvTests(unittest.TestCase):
    def test_sets_env_vars_from_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            env_path = Path(tmpdir) / ".env"
            env_path.write_text("MODEL_FIX_TEST_KEY=hello\n")
            with patch.dict(os.environ, {}, clear=False):
                os.environ.pop("MODEL_FIX_TEST_KEY", None)
                _load_dotenv(env_path)
                self.assertEqual(os.environ["MODEL_FIX_TEST_KEY"], "hello")

    def test_skips_comments_and_blank_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            env_path = Path(tmpdir) / ".env"
            env_path.write_text("# a comment\n\nMODEL_FIX_TEST_KEY2=world\n")
            with patch.dict(os.environ, {}, clear=False):
                os.environ.pop("MODEL_FIX_TEST_KEY2", None)
                _load_dotenv(env_path)
                self.assertEqual(os.environ["MODEL_FIX_TEST_KEY2"], "world")

    def test_does_not_override_existing_env_var(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            env_path = Path(tmpdir) / ".env"
            env_path.write_text("MODEL_FIX_TEST_KEY3=from_file\n")
            with patch.dict(os.environ, {"MODEL_FIX_TEST_KEY3": "from_shell"}, clear=False):
                _load_dotenv(env_path)
                self.assertEqual(os.environ["MODEL_FIX_TEST_KEY3"], "from_shell")

    def test_missing_file_is_a_silent_no_op(self):
        _load_dotenv(Path("/nonexistent/path/.env"))  # must not raise


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

    def test_tolerates_trailing_space_and_crlf_after_fence(self):
        text = "```diff \r\n--- a/foo.rs\r\n+++ b/foo.rs\r\n```\n"
        diff = extract_diff(text)
        self.assertIsNotNone(diff)
        self.assertTrue(diff.startswith("--- a/foo.rs"))


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
            max_tokens=4096,
            reasoning_effort="max",
        )

        self.assertEqual(result, "the diff")
        request = mock_urlopen.call_args[0][0]
        self.assertEqual(request.full_url, "https://api.z.ai/api/paas/v4/chat/completions")
        self.assertEqual(request.get_header("Authorization"), "Bearer secret")
        body = json.loads(request.data)
        self.assertEqual(body["model"], "glm-5.2")
        self.assertEqual(body["messages"], [{"role": "user", "content": "fix it"}])
        self.assertEqual(body["max_tokens"], 4096)
        self.assertEqual(body["reasoning_effort"], "max")


class CallModelStreamingTests(unittest.TestCase):
    @patch("model_fix_loop.urllib.request.urlopen")
    def test_stream_true_sets_stream_field_in_request_body(self, mock_urlopen):
        mock_cm = MagicMock()
        mock_cm.__iter__.return_value = iter([b"data: [DONE]\n"])
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
            stream=True,
        )

        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertTrue(body["stream"])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_stream_false_by_default(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        mock_cm = MagicMock()
        mock_cm.read.return_value = response_json
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
        )

        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertFalse(body["stream"])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_reassembles_sse_chunks_into_full_reply(self, mock_urlopen):
        lines = [
            b'data: {"choices":[{"delta":{"content":"Hello"}}]}\n',
            b'data: {"choices":[{"delta":{"content":", world"}}]}\n',
            b'data: {"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}\n',
            b"data: [DONE]\n",
        ]
        mock_cm = MagicMock()
        mock_cm.__iter__.return_value = iter(lines)
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        result = call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
            stream=True,
        )

        self.assertEqual(result, "Hello, world")

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_skips_chunks_with_no_content_delta(self, mock_urlopen):
        lines = [
            b'data: {"choices":[{"delta":{}}]}\n',
            b'data: {"choices":[{"delta":{"content":"ok"}}]}\n',
            b"data: [DONE]\n",
        ]
        mock_cm = MagicMock()
        mock_cm.__iter__.return_value = iter(lines)
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        result = call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
            stream=True,
        )

        self.assertEqual(result, "ok")


class CallModelThinkingTests(unittest.TestCase):
    @patch("model_fix_loop.urllib.request.urlopen")
    def test_thinking_true_by_default_omits_thinking_field(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        mock_cm = MagicMock()
        mock_cm.read.return_value = response_json
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
        )

        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertNotIn("thinking", body)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_thinking_false_sends_disabled_thinking_field(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        mock_cm = MagicMock()
        mock_cm.read.return_value = response_json
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
            thinking=False,
        )

        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertEqual(body["thinking"], {"type": "disabled"})


class CallModelTemperatureTests(unittest.TestCase):
    @patch("model_fix_loop.urllib.request.urlopen")
    def test_temperature_zero_by_default(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        mock_cm = MagicMock()
        mock_cm.read.return_value = response_json
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
        )

        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertEqual(body["temperature"], 0)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_custom_temperature_is_sent(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        mock_cm = MagicMock()
        mock_cm.read.return_value = response_json
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
            max_tokens=4096,
            reasoning_effort="max",
            temperature=0.7,
        )

        request = mock_urlopen.call_args[0][0]
        body = json.loads(request.data)
        self.assertEqual(body["temperature"], 0.7)


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


def make_gap(gap_count=2):
    return {
        "format": "NEF",
        "missing_tags": [
            {"family": "EXIF", "name": "LensModel", "value": "50mm", "tag_id": None, "source_file": "a.nef"}
        ],
        "value_differences": [
            {"tag_key": "EXIF:ISO", "exiftool_value": "100", "oxidex_value": "0", "source_file": "a.nef"}
        ],
        "gap_count": gap_count,
        "parser_files": [],
    }


class BuildPromptTests(unittest.TestCase):
    def test_caps_missing_tags_and_notes_the_omitted_count(self):
        gap = {
            "format": "JPEG",
            "missing_tags": [
                {"family": "EXIF", "name": f"Tag{i}", "value": "x", "tag_id": None, "source_file": None}
                for i in range(5)
            ],
            "value_differences": [],
            "gap_count": 5,
            "parser_files": [],
        }
        prompt = build_prompt(gap, max_tags=2, max_file_bytes=1000)
        self.assertIn("Tag0", prompt)
        self.assertIn("Tag1", prompt)
        self.assertNotIn("Tag2", prompt)
        self.assertIn("3 more, not shown", prompt)

    def test_caps_parser_file_bytes_but_always_includes_at_least_one_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "big.rs").write_text("x" * 100)
            (tmp / "small.rs").write_text("y" * 10)
            gap = {
                "format": "JPEG",
                "missing_tags": [],
                "value_differences": [],
                "gap_count": 0,
                "parser_files": ["big.rs", "small.rs"],
            }
            prompt = build_prompt(gap, repo_root=tmp, max_tags=40, max_file_bytes=50)
            self.assertIn("big.rs", prompt)
            self.assertNotIn("small.rs", prompt)
            self.assertIn("1 additional file(s) omitted", prompt)

    def test_no_truncation_notes_when_everything_fits(self):
        gap = make_gap(gap_count=2)
        prompt = build_prompt(gap, max_tags=40, max_file_bytes=60_000)
        self.assertNotIn("more, not shown", prompt)
        self.assertNotIn("additional file(s) omitted", prompt)


class BuildReviewPromptTests(unittest.TestCase):
    def test_includes_diff_and_tag_names(self):
        gap = make_gap(gap_count=2)  # missing: EXIF:LensModel; diff: EXIF:ISO
        prompt = build_review_prompt(gap, "--- a/x\n+++ b/x\n")
        self.assertIn("--- a/x", prompt)
        self.assertIn("EXIF:LensModel", prompt)
        self.assertIn("EXIF:ISO", prompt)
        self.assertIn("NEF", prompt)


class ExtractReviewVerdictTests(unittest.TestCase):
    def test_approve(self):
        self.assertEqual(extract_review_verdict("APPROVE"), (True, ""))

    def test_approve_case_insensitive_with_trailing_text(self):
        approved, reason = extract_review_verdict("approve\nLooks correct.")
        self.assertTrue(approved)

    def test_reject_with_reason(self):
        approved, reason = extract_review_verdict("REJECT: hardcodes the sample's literal value")
        self.assertFalse(approved)
        self.assertEqual(reason, "hardcodes the sample's literal value")

    def test_unparseable_defaults_to_rejected(self):
        approved, reason = extract_review_verdict("I'm not sure about this one.")
        self.assertFalse(approved)
        self.assertIn("unparseable review verdict", reason)


class ReviewVerdictTests(unittest.TestCase):
    def test_parses_approval_from_call_model(self):
        gap = make_gap()
        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "model": "glm-5.2", "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=lambda messages, *a: "APPROVE",
        )
        self.assertTrue(approved)

    def test_parses_rejection_from_call_model(self):
        gap = make_gap()
        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "model": "glm-5.2", "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=lambda messages, *a: "REJECT: hardcoded value",
        )
        self.assertFalse(approved)
        self.assertEqual(reason, "hardcoded value")

    def test_treats_call_failure_as_rejection(self):
        gap = make_gap()

        def raising(messages, *a):
            raise TimeoutError("timed out")

        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "model": "glm-5.2", "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=raising,
        )
        self.assertFalse(approved)
        self.assertIn("review call failed", reason)


class FixGapHappyPathTests(unittest.TestCase):
    def test_commits_when_build_and_tests_pass_and_gaps_shrink(self):
        gap = make_gap(gap_count=2)
        model_calls = []
        commit_calls = []

        result = fix_gap(
            gap,
            {
                "base_url": "u", "api_key": "k", "model": "glm-5.2",
                "max_tokens": 4096, "reasoning_effort": "max",
                "max_prompt_tags": 40, "max_prompt_file_bytes": 60_000,
            },
            call_model_fn=lambda messages, *a: (model_calls.append(1), "```diff\n--- a/x\n+++ b/x\n```\n")[1],
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: commit_calls.append(msg),
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: True,
            review_fn=lambda g, diff, config, **kwargs: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(result["gaps_closed"], 2)
        self.assertEqual(len(model_calls), 1)
        self.assertEqual(len(commit_calls), 1)
        self.assertIn("glm-5.2", commit_calls[0])


CONFIG = {
    "base_url": "u", "api_key": "k", "model": "glm-5.2",
    "max_tokens": 4096, "reasoning_effort": "max",
    "max_prompt_tags": 40, "max_prompt_file_bytes": 60_000,
}


class AttemptBuildTests(unittest.TestCase):
    def test_builds_on_first_attempt(self):
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertIsNone(reason)
        self.assertTrue(diff.startswith("--- a/x"))

    def test_retries_once_on_build_failure_then_succeeds(self):
        build_attempts = []

        def fake_cargo_build(root):
            build_attempts.append(1)
            if len(build_attempts) == 1:
                return False, "error[E0308]: mismatched types"
            return True, ""

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=fake_cargo_build,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(build_attempts), 2)

    def test_retries_once_on_apply_failure_then_succeeds(self):
        apply_attempts = []

        def fake_git_apply(diff, root):
            apply_attempts.append(1)
            if len(apply_attempts) == 1:
                return False, "patch does not apply"
            return True, "ok"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=fake_git_apply,
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(apply_attempts), 2)

    def test_fails_after_two_build_failures(self):
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (False, "still broken"),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertEqual(reason, "no working fix after repair attempt")
        self.assertIsNone(diff)

    def test_fails_when_no_diff_in_response(self):
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "I could not find a fix.",
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertEqual(reason, "no diff in model response")

    def test_fails_gracefully_when_model_call_raises(self):
        def raising_call_model(messages, *a):
            raise TimeoutError("The read operation timed out")

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=raising_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertIn("model call failed", reason)
        self.assertIn("timed out", reason)


class FixGapFailureTests(unittest.TestCase):
    def test_fails_when_gap_count_does_not_decrease(self):
        gap = make_gap(gap_count=2)
        result = fix_gap(
            gap,
            {
                "base_url": "u", "api_key": "k", "model": "glm-5.2",
                "max_tokens": 4096, "reasoning_effort": "max",
                "max_prompt_tags": 40, "max_prompt_file_bytes": 60_000,
            },
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 2,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["reason"], "gap count did not decrease")

    def test_fails_when_tests_regress(self):
        gap = make_gap(gap_count=2)
        result = fix_gap(
            gap,
            {
                "base_url": "u", "api_key": "k", "model": "glm-5.2",
                "max_tokens": 4096, "reasoning_effort": "max",
                "max_prompt_tags": 40, "max_prompt_file_bytes": 60_000,
            },
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: False,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["reason"], "cargo test --workspace regressed")


class FixGapReviewTests(unittest.TestCase):
    def test_retries_once_when_review_rejects_then_approves(self):
        gap = make_gap(gap_count=2)
        review_calls = []
        attempt_calls = []
        commit_calls = []

        def fake_attempt_build(messages, **kwargs):
            attempt_calls.append(len(messages))
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        def fake_review(g, diff, config, **kwargs):
            review_calls.append(1)
            if len(review_calls) == 1:
                return False, "hardcodes the sample value"
            return True, ""

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            review_fn=fake_review,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: commit_calls.append(msg),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(review_calls), 2)
        self.assertEqual(len(attempt_calls), 2)
        self.assertGreater(attempt_calls[1], attempt_calls[0])
        self.assertEqual(len(commit_calls), 1)

    def test_fails_after_review_rejects_twice(self):
        gap = make_gap(gap_count=2)

        def fake_attempt_build(messages, **kwargs):
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            review_fn=lambda g, diff, config, **kwargs: (False, "hardcodes the sample value"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "failed")
        self.assertIn("rejected by review after repair attempt", result["reason"])
        self.assertIn("hardcodes the sample value", result["reason"])

    def test_review_uses_fix_gaps_injected_call_model_fn(self):
        gap = make_gap(gap_count=2)
        review_call_model_calls = []

        def fake_attempt_build(messages, **kwargs):
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        def tracking_call_model_fn(messages, *a):
            review_call_model_calls.append(messages)
            return "APPROVE"

        result = fix_gap(
            gap, CONFIG,
            call_model_fn=tracking_call_model_fn,
            attempt_build_fn=fake_attempt_build,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(review_call_model_calls), 1)

    def test_config_stream_flag_reaches_call_model_fn(self):
        gap = make_gap(gap_count=2)
        stream_values_seen = []

        def tracking_call_model_fn(messages, base_url, api_key, model, max_tokens, reasoning_effort,
                                    stream=False, thinking=True, temperature=0):
            stream_values_seen.append(stream)
            if len(stream_values_seen) == 1:
                return "```diff\n--- a/x\n+++ b/x\n```\n"
            return "APPROVE"

        config = dict(CONFIG, stream=True)
        result = fix_gap(
            gap, config,
            call_model_fn=tracking_call_model_fn,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        # Both attempt_build's fixer call and review_verdict's call must
        # see the config's stream flag -- proving it's threaded through
        # both real call sites, not just one of them.
        self.assertEqual(stream_values_seen, [True, True])

    def test_config_thinking_flag_reaches_call_model_fn(self):
        gap = make_gap(gap_count=2)
        thinking_values_seen = []

        def tracking_call_model_fn(messages, base_url, api_key, model, max_tokens, reasoning_effort,
                                    stream=False, thinking=True, temperature=0):
            thinking_values_seen.append(thinking)
            if len(thinking_values_seen) == 1:
                return "```diff\n--- a/x\n+++ b/x\n```\n"
            return "APPROVE"

        config = dict(CONFIG, thinking=False)
        result = fix_gap(
            gap, config,
            call_model_fn=tracking_call_model_fn,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(thinking_values_seen, [False, False])

    def test_config_temperature_flag_reaches_call_model_fn(self):
        gap = make_gap(gap_count=2)
        temperature_values_seen = []

        def tracking_call_model_fn(messages, base_url, api_key, model, max_tokens, reasoning_effort,
                                    stream=False, thinking=True, temperature=0):
            temperature_values_seen.append(temperature)
            if len(temperature_values_seen) == 1:
                return "```diff\n--- a/x\n+++ b/x\n```\n"
            return "APPROVE"

        config = dict(CONFIG, temperature=0.7)
        result = fix_gap(
            gap, config,
            call_model_fn=tracking_call_model_fn,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(temperature_values_seen, [0.7, 0.7])

    def test_uses_separate_review_config_when_provided(self):
        gap = make_gap(gap_count=2)
        configs_seen = []

        def fake_attempt_build(messages, **kwargs):
            configs_seen.append(("fixer", kwargs["config"]))
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        def fake_review(g, diff, config, **kwargs):
            configs_seen.append(("review", config))
            return True, ""

        review_config = dict(CONFIG, model="review-model", base_url="https://review.example/v1")

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            review_fn=fake_review,
            review_config=review_config,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        fixer_config = next(c for label, c in configs_seen if label == "fixer")
        review_seen_config = next(c for label, c in configs_seen if label == "review")
        self.assertEqual(fixer_config["model"], "glm-5.2")
        self.assertEqual(review_seen_config["model"], "review-model")
        self.assertEqual(review_seen_config["base_url"], "https://review.example/v1")

    def test_review_config_defaults_to_fixer_config_when_not_provided(self):
        gap = make_gap(gap_count=2)
        seen_review_config = []

        def fake_attempt_build(messages, **kwargs):
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        def fake_review(g, diff, config, **kwargs):
            seen_review_config.append(config)
            return True, ""

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            review_fn=fake_review,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(seen_review_config[0], CONFIG)


class RunLoopTests(unittest.TestCase):
    def test_stops_after_two_consecutive_dry_rounds(self):
        find_calls = []

        def fake_find_gaps():
            find_calls.append(1)
            return []

        result = run_loop({"model": "x"}, fake_find_gaps, fix_gap_fn=lambda g, c: self.fail("should not fix"))
        self.assertEqual(result["rounds"], 2)
        self.assertEqual(len(find_calls), 2)

    def test_resets_dry_streak_when_a_gap_closes(self):
        rounds = [[make_gap()], [], []]

        def fake_find_gaps():
            return rounds.pop(0)

        def fake_fix_gap(gap, config):
            return {"format": gap["format"], "status": "fixed", "gaps_closed": gap["gap_count"]}

        result = run_loop({"model": "x"}, fake_find_gaps, fake_fix_gap)
        self.assertEqual(result["rounds"], 3)
        self.assertEqual(len(result["fixed"]), 1)

    def test_skips_a_format_that_fails_twice(self):
        nef_gap = make_gap()  # format "NEF"
        other_gap = {
            "format": "PNG",
            "missing_tags": [],
            "value_differences": [],
            "gap_count": 1,
            "parser_files": [],
        }
        attempts = []
        # Round 1: NEF fails (1st failure). Round 2: NEF fails again (2nd
        # failure -> skip-listed) and PNG closes (keeps dry_rounds at 0, so
        # the loop survives into round 3). Round 3: NEF must be filtered
        # out by the skip-list and never dispatched again; PNG has nothing
        # left, so round 3 is dry and the loop stops after round 4 (dry
        # again) via the 2-consecutive-dry-round rule.
        rounds = [
            [nef_gap],
            [nef_gap, other_gap],
            [nef_gap],  # would only appear here if the skip-list filter is broken
            [],
        ]

        def fake_find_gaps():
            return rounds.pop(0) if rounds else []

        def fake_fix_gap(g, config):
            attempts.append(g["format"])
            if g["format"] == "PNG":
                return {"format": "PNG", "status": "fixed", "gaps_closed": g["gap_count"]}
            return {"format": g["format"], "status": "failed", "reason": "still broken"}

        result = run_loop({"model": "x"}, fake_find_gaps, fake_fix_gap)

        # NEF attempted exactly twice (rounds 1 and 2), never a third time,
        # even though round 3's fake data includes it -- proving the
        # skip-list filter in run_loop actually removes it before dispatch.
        self.assertEqual(attempts.count("NEF"), 2)
        self.assertEqual(result["skipped"], ["NEF"])


if __name__ == "__main__":
    unittest.main()
