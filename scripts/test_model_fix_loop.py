import json
import tempfile
import time
import unittest
import urllib.error
from unittest.mock import patch, MagicMock
from pathlib import Path

from model_fix_loop import (
    _normalize_model_config,
    attempt_build,
    build_exact_sample_block,
    build_prompt,
    build_review_prompt,
    cargo_build,
    cargo_test_workspace,
    call_model,
    detect_duplicate_tag_insertion,
    expand_gaps_to_tags,
    extract_diff,
    extract_review_verdict,
    file_content_at_head,
    fix_gap,
    git_apply,
    git_checkout_clean,
    git_commit,
    load_toml_config,
    make_single_tag_gap,
    refresh_worktree,
    review_verdict,
    run_loop,
    run_tag_loop,
    tag_key_for,
    tag_literal_for_gap,
)


class LoadTomlConfigTests(unittest.TestCase):
    def test_parses_worker_and_reviewer_tables(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = Path(tmpdir) / "config.toml"
            config_path.write_text(
                '[worker]\nbase_url = "https://api.example/v1"\napi_key = "k"\n'
                'models = ["a", "b"]\n\n'
                '[reviewer]\nmodels = ["c"]\n'
            )
            data = load_toml_config(config_path)
            self.assertEqual(data["worker"]["models"], ["a", "b"])
            self.assertEqual(data["reviewer"]["models"], ["c"])

    def test_missing_file_returns_none(self):
        self.assertIsNone(load_toml_config(Path("/nonexistent/path/config.toml")))


class NormalizeModelConfigTests(unittest.TestCase):
    def test_fills_in_defaults_for_missing_keys(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["max_tokens"], 4096)
        self.assertEqual(config["reasoning_effort"], "max")
        self.assertEqual(config["stream"], False)
        self.assertEqual(config["thinking"], True)
        self.assertEqual(config["temperature"], 0)

    def test_preserves_explicit_values(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m1", "m2"],
            "max_tokens": 16, "reasoning_effort": "low", "stream": True,
            "thinking": False, "temperature": 0.7,
        })
        self.assertEqual(config["models"], [
            {"name": "m1", "base_url": "u", "api_key": "k"},
            {"name": "m2", "base_url": "u", "api_key": "k"},
        ])
        self.assertEqual(config["max_tokens"], 16)
        self.assertEqual(config["stream"], True)

    def test_string_model_entries_inherit_table_base_url_and_api_key(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["models"], [{"name": "m", "base_url": "u", "api_key": "k"}])

    def test_table_model_entries_can_override_base_url_and_api_key(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k",
            "models": [
                "shared-provider-model",
                {"name": "other-provider-model", "base_url": "https://other.example/v1", "api_key": "other-key"},
            ],
        })
        self.assertEqual(config["models"], [
            {"name": "shared-provider-model", "base_url": "u", "api_key": "k"},
            {"name": "other-provider-model", "base_url": "https://other.example/v1", "api_key": "other-key"},
        ])

    def test_rejects_unrecognized_keys_on_a_models_entry_instead_of_silently_dropping_them(self):
        # This exact shape -- max_tokens misplaced under a models[] entry
        # instead of the parent table -- silently no-op'd instead of
        # erroring, so a real run's configured max_tokens/temperature/etc.
        # never took effect and nothing reported it.
        with self.assertRaises(ValueError) as ctx:
            _normalize_model_config({
                "base_url": "u", "api_key": "k",
                "models": [{"name": "glm5.2-fast", "max_tokens": 1024, "temperature": 0.8}],
            })
        self.assertIn("max_tokens", str(ctx.exception))
        self.assertIn("glm5.2-fast", str(ctx.exception))


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


class CallModelRetryTests(unittest.TestCase):
    def _http_error(self, code):
        return urllib.error.HTTPError(
            url="https://api.example/v1/chat/completions", code=code,
            msg="err", hdrs=None, fp=None,
        )

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_retries_on_5xx_then_succeeds(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        ok_cm = MagicMock()
        ok_cm.read.return_value = response_json
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [self._http_error(502), self._http_error(500), ok_ctx]

        sleeps = []
        result = call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.example/v1", api_key="k", model="m",
            max_tokens=100, reasoning_effort="max",
            sleep_fn=sleeps.append,
        )
        self.assertEqual(result, "the diff")
        self.assertEqual(mock_urlopen.call_count, 3)
        # Exponential: 2s then 4s.
        self.assertEqual(sleeps, [2, 4])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_retries_on_connection_level_url_error_then_succeeds(self, mock_urlopen):
        # A DNS failure (or refused connection/TLS handshake/stalled read)
        # raises urllib.error.URLError, not HTTPError -- no HTTP response
        # was ever received at all. Previously only HTTPError was caught
        # here, so this propagated straight past call_model's retry loop
        # on the very first attempt: confirmed live, a DNS outage burned
        # all 10 of one tag's fail-count attempts and got it blacklisted
        # without the model ever actually being reachable.
        dns_failure = urllib.error.URLError(
            OSError(8, "nodename nor servname provided, or not known")
        )
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        ok_cm = MagicMock()
        ok_cm.read.return_value = response_json
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [dns_failure, dns_failure, ok_ctx]

        sleeps = []
        result = call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.example/v1", api_key="k", model="m",
            max_tokens=100, reasoning_effort="max",
            sleep_fn=sleeps.append,
        )
        self.assertEqual(result, "the diff")
        self.assertEqual(mock_urlopen.call_count, 3)
        self.assertEqual(sleeps, [2, 4])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_gives_up_after_max_retries_on_persistent_url_error(self, mock_urlopen):
        mock_urlopen.side_effect = urllib.error.URLError(OSError(8, "Could not resolve host"))
        with self.assertRaises(urllib.error.URLError):
            call_model(
                [{"role": "user", "content": "fix it"}],
                base_url="https://api.example/v1", api_key="k", model="m",
                max_tokens=100, reasoning_effort="max",
                max_retries=2, sleep_fn=lambda s: None,
            )
        self.assertEqual(mock_urlopen.call_count, 3)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_logs_each_retry_so_a_long_ride_out_is_not_silent(self, mock_urlopen):
        # A worker riding out many transient failures (the whole point of
        # a high max_retries) must not go completely silent for however
        # long that takes -- previously nothing was logged per retry,
        # making a busy worker indistinguishable from a stuck one on any
        # dashboard/log tailing it.
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        ok_cm = MagicMock()
        ok_cm.read.return_value = response_json
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [self._http_error(502), self._http_error(500), ok_ctx]

        logged = []
        call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.example/v1", api_key="k", model="m",
            max_tokens=100, reasoning_effort="max",
            sleep_fn=lambda s: None, log_fn=logged.append,
        )
        self.assertEqual(len(logged), 2)
        self.assertIn("retry 1/", logged[0])
        self.assertIn("retry 2/", logged[1])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_does_not_retry_on_4xx(self, mock_urlopen):
        mock_urlopen.side_effect = self._http_error(400)
        with self.assertRaises(urllib.error.HTTPError):
            call_model(
                [{"role": "user", "content": "fix it"}],
                base_url="https://api.example/v1", api_key="k", model="m",
                max_tokens=100, reasoning_effort="max",
                sleep_fn=lambda s: self.fail("must not sleep/retry on a 4xx"),
            )
        self.assertEqual(mock_urlopen.call_count, 1)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_retries_on_empty_reply_then_succeeds(self, mock_urlopen):
        empty_cm = MagicMock()
        empty_cm.read.return_value = json.dumps({"choices": [{"message": {"content": ""}}]}).encode()
        empty_ctx = MagicMock()
        empty_ctx.__enter__.return_value = empty_cm

        ok_cm = MagicMock()
        ok_cm.read.return_value = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm

        mock_urlopen.side_effect = [empty_ctx, ok_ctx]
        result = call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.example/v1", api_key="k", model="m",
            max_tokens=100, reasoning_effort="max",
            sleep_fn=lambda s: None,
        )
        self.assertEqual(result, "the diff")
        self.assertEqual(mock_urlopen.call_count, 2)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_gives_up_after_max_retries(self, mock_urlopen):
        mock_urlopen.side_effect = self._http_error(503)
        with self.assertRaises(urllib.error.HTTPError):
            call_model(
                [{"role": "user", "content": "fix it"}],
                base_url="https://api.example/v1", api_key="k", model="m",
                max_tokens=100, reasoning_effort="max",
                max_retries=2, sleep_fn=lambda s: None,
            )
        # 1 initial attempt + 2 retries = 3 calls total.
        self.assertEqual(mock_urlopen.call_count, 3)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_backoff_growth_is_capped(self, mock_urlopen):
        mock_urlopen.side_effect = self._http_error(500)
        sleeps = []
        with self.assertRaises(urllib.error.HTTPError):
            call_model(
                [{"role": "user", "content": "fix it"}],
                base_url="https://api.example/v1", api_key="k", model="m",
                max_tokens=100, reasoning_effort="max",
                max_retries=5, retry_backoff_seconds=10, max_retry_backoff_seconds=25,
                sleep_fn=sleeps.append,
            )
        # 10, 20, capped at 25, 25, 25 -- never allowed to keep doubling
        # past max_retry_backoff_seconds.
        self.assertEqual(sleeps, [10, 20, 25, 25, 25])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_max_retries_default_is_high_not_unlimited(self, mock_urlopen):
        mock_urlopen.side_effect = self._http_error(500)
        with self.assertRaises(urllib.error.HTTPError):
            call_model(
                [{"role": "user", "content": "fix it"}],
                base_url="https://api.example/v1", api_key="k", model="m",
                max_tokens=100, reasoning_effort="max",
                max_retry_backoff_seconds=0, sleep_fn=lambda s: None,
            )
        # Default max_retries=1000 -> 1001 calls total, not infinite.
        self.assertEqual(mock_urlopen.call_count, 1001)

    def test_max_retries_below_zero_raises_clear_error_not_typeerror(self):
        # range(max_retries + 1) never iterates when max_retries < 0, so
        # last_error is still None at the final `raise` -- must not
        # `raise None` (TypeError masking the real misconfiguration).
        # No urlopen mock needed: a real attempt would fail differently
        # (network error), which would also correctly fail this test.
        with self.assertRaises(RuntimeError):
            call_model(
                [{"role": "user", "content": "fix it"}],
                base_url="https://api.example/v1", api_key="k", model="m",
                max_tokens=100, reasoning_effort="max",
                max_retries=-1, sleep_fn=lambda s: None,
            )


class CallModelStreamingTests(unittest.TestCase):
    @patch("model_fix_loop.urllib.request.urlopen")
    def test_stream_true_sets_stream_field_in_request_body(self, mock_urlopen):
        mock_cm = MagicMock()
        mock_cm.__iter__.return_value = iter([
            b'data: {"choices":[{"delta":{"content":"hi"}}]}\n',
            b"data: [DONE]\n",
        ])
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
        self.assertEqual(args[0], ["git", "apply", "--reject", "--recount", "-"])
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


class RefreshWorktreeTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_clean_fast_forward_returns_true(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="Updating abc..def\n", stderr="")
        ok, message = refresh_worktree(Path("/fake/repo"), "shared-branch")
        self.assertTrue(ok)
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["git", "merge", "--ff-only", "shared-branch"])
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))

    @patch("model_fix_loop.subprocess.run")
    def test_already_up_to_date_returns_true(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="Already up to date.\n", stderr="")
        ok, message = refresh_worktree(Path("/fake/repo"), "shared-branch")
        self.assertTrue(ok)

    @patch("model_fix_loop.subprocess.run")
    def test_non_fast_forward_returns_false_with_message(self, mock_run):
        # The rare case this is designed to bail out of rather than risk
        # a real merge conflict deep inside a retry loop -- see
        # refresh_worktree's own docstring for why this "shouldn't"
        # happen under --max-tags-per-process=1, but must still degrade
        # safely (skip this round's refresh) if it ever does.
        mock_run.return_value = MagicMock(
            returncode=128, stdout="", stderr="fatal: Not possible to fast-forward, aborting.\n",
        )
        ok, message = refresh_worktree(Path("/fake/repo"), "shared-branch")
        self.assertFalse(ok)
        self.assertIn("Not possible to fast-forward", message)


class FileContentAtHeadTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_existing_path_returns_its_head_content(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="fn foo() {}\n")
        content = file_content_at_head("src/foo.rs", Path("/fake/repo"))
        self.assertEqual(content, "fn foo() {}\n")
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["git", "show", "HEAD:src/foo.rs"])
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))

    @patch("model_fix_loop.subprocess.run")
    def test_path_not_at_head_returns_empty_string(self, mock_run):
        # A brand-new file the diff itself creates -- nothing to have
        # already duplicated at HEAD.
        mock_run.return_value = MagicMock(returncode=128, stdout="", stderr="fatal: path does not exist")
        content = file_content_at_head("src/new.rs", Path("/fake/repo"))
        self.assertEqual(content, "")


class TagLiteralForGapTests(unittest.TestCase):
    def test_missing_tag_combines_family_and_name(self):
        gap = {"missing_tags": [{"family": "APP12", "name": "CAM1"}], "value_differences": []}
        self.assertEqual(tag_literal_for_gap(gap), '"APP12:CAM1"')

    def test_value_difference_uses_its_own_tag_key(self):
        gap = {"missing_tags": [], "value_differences": [{"tag_key": "EXIF:ISO"}]}
        self.assertEqual(tag_literal_for_gap(gap), '"EXIF:ISO"')

    def test_zero_entries_returns_none(self):
        self.assertIsNone(tag_literal_for_gap({"missing_tags": [], "value_differences": []}))

    def test_multiple_entries_returns_none(self):
        # Skip the check rather than guess which of several tags in a
        # (non-single-tag) gap a diff was meant to address.
        gap = {
            "missing_tags": [
                {"family": "APP12", "name": "CAM1"}, {"family": "APP12", "name": "CAM2"},
            ],
            "value_differences": [],
        }
        self.assertIsNone(tag_literal_for_gap(gap))


class DetectDuplicateTagInsertionTests(unittest.TestCase):
    DIFF_HEADER = (
        "diff --git a/src/foo.rs b/src/foo.rs\n"
        "index 1111111..2222222 100644\n"
        "--- a/src/foo.rs\n"
        "+++ b/src/foo.rs\n"
    )

    def _write_current(self, tmpdir, text):
        path = Path(tmpdir) / "src" / "foo.rs"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    @patch("model_fix_loop.subprocess.run")
    def test_brand_new_tag_is_not_a_duplicate(self, mock_run):
        # Occurrence count 0 -> 1: genuinely new, the common successful case.
        mock_run.return_value = MagicMock(returncode=0, stdout="fn parse() {}\n")
        with tempfile.TemporaryDirectory() as tmpdir:
            self._write_current(tmpdir, 'fn parse() {\n    metadata.insert("APP12:CAM1", v);\n}\n')
            result = detect_duplicate_tag_insertion(
                self.DIFF_HEADER + '+    metadata.insert("APP12:CAM1", v);\n', '"APP12:CAM1"', tmpdir,
            )
            self.assertFalse(result)

    @patch("model_fix_loop.subprocess.run")
    def test_in_place_edit_is_not_a_duplicate(self, mock_run):
        # Occurrence count 1 -> 1: the existing occurrence was edited
        # (old value removed, new value added), not duplicated.
        mock_run.return_value = MagicMock(
            returncode=0, stdout='fn parse() {\n    metadata.insert("APP12:CAM1", old_v);\n}\n',
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            self._write_current(tmpdir, 'fn parse() {\n    metadata.insert("APP12:CAM1", new_v);\n}\n')
            result = detect_duplicate_tag_insertion(
                self.DIFF_HEADER
                + '-    metadata.insert("APP12:CAM1", old_v);\n'
                + '+    metadata.insert("APP12:CAM1", new_v);\n',
                '"APP12:CAM1"', tmpdir,
            )
            self.assertFalse(result)

    @patch("model_fix_loop.subprocess.run")
    def test_redundant_second_occurrence_is_a_duplicate(self, mock_run):
        # Occurrence count 1 -> 2: a new occurrence added ALONGSIDE an
        # untouched existing one -- exactly the shape of every merge
        # conflict this pipeline has hit so far.
        mock_run.return_value = MagicMock(
            returncode=0, stdout='fn parse() {\n    metadata.insert("APP12:CAM1", v);\n}\n',
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            self._write_current(
                tmpdir,
                'fn parse() {\n    metadata.insert("APP12:CAM1", v);\n'
                '    metadata.insert("APP12:CAM1", v2);\n}\n',
            )
            result = detect_duplicate_tag_insertion(
                self.DIFF_HEADER + '+    metadata.insert("APP12:CAM1", v2);\n', '"APP12:CAM1"', tmpdir,
            )
            self.assertTrue(result)

    @patch("model_fix_loop.subprocess.run")
    def test_different_tags_sharing_a_file_do_not_interfere(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout='metadata.insert("APP12:CAM2", v);\n')
        with tempfile.TemporaryDirectory() as tmpdir:
            self._write_current(
                tmpdir, 'metadata.insert("APP12:CAM2", v);\nmetadata.insert("APP12:CAM1", v);\n',
            )
            result = detect_duplicate_tag_insertion(
                self.DIFF_HEADER + '+metadata.insert("APP12:CAM1", v);\n', '"APP12:CAM1"', tmpdir,
            )
            self.assertFalse(result)

    def test_diff_with_no_file_headers_returns_false(self):
        self.assertFalse(detect_duplicate_tag_insertion("not a real diff", '"APP12:CAM1"', "/fake/repo"))


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


def make_single_tag_gap_dict(source_file=None):
    entry = {"family": "APP0", "name": "OcadRevision", "value": "1", "tag_id": None, "source_file": source_file}
    return {
        "format": "JPEG", "missing_tags": [entry], "value_differences": [], "gap_count": 1, "parser_files": [],
    }


class BuildExactSampleBlockTests(unittest.TestCase):
    def test_returns_empty_when_gap_has_more_than_one_tag(self):
        gap = make_gap(gap_count=2)  # 2 tags total (1 missing + 1 diff)
        self.assertEqual(build_exact_sample_block(gap, None), "")

    def test_returns_empty_when_source_file_is_none(self):
        gap = make_single_tag_gap_dict(source_file=None)
        self.assertEqual(build_exact_sample_block(gap, None), "")

    def test_returns_empty_when_source_file_does_not_exist(self):
        gap = make_single_tag_gap_dict(source_file="/nonexistent/file.jpg")
        self.assertEqual(build_exact_sample_block(gap, None), "")

    def test_inlines_full_hex_dump_for_a_small_sample(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "small.jpg"
            path.write_bytes(b"\xff\xd8\xff\xe0hello")
            gap = make_single_tag_gap_dict(source_file=str(path))
            block = build_exact_sample_block(gap, tmpdir)
            self.assertIn("small.jpg", block)
            self.assertIn("full hex dump", block)
            self.assertIn("ff d8 ff e0", block)
            self.assertNotIn("REQUEST:", block)

    def test_flags_path_and_size_instead_of_inlining_a_large_sample(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "big.jpg"
            path.write_bytes(b"x" * 5000)
            gap = make_single_tag_gap_dict(source_file=str(path))
            block = build_exact_sample_block(gap, tmpdir)
            self.assertIn("big.jpg", block)
            self.assertIn("5000 bytes", block)
            self.assertIn("too large to inline", block)
            self.assertIn('REQUEST: big.jpg', block)

    def test_shows_path_relative_to_samples_dir_when_possible(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            sub = Path(tmpdir) / "Sony"
            sub.mkdir()
            path = sub / "camera.jpg"
            path.write_bytes(b"x" * 10)
            gap = make_single_tag_gap_dict(source_file=str(path))
            block = build_exact_sample_block(gap, tmpdir)
            self.assertIn("Sony/camera.jpg", block)

    def test_falls_back_to_absolute_path_when_not_under_samples_dir(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "elsewhere.jpg"
            path.write_bytes(b"x" * 10)
            gap = make_single_tag_gap_dict(source_file=str(path))
            block = build_exact_sample_block(gap, "/some/other/samples/dir")
            self.assertIn(str(path), block)


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
            {"base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}], "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=lambda messages, *a: "APPROVE",
        )
        self.assertTrue(approved)

    def test_parses_rejection_from_call_model(self):
        gap = make_gap()
        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}], "max_tokens": 4096, "reasoning_effort": "max"},
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
            {"base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}], "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=raising,
        )
        self.assertFalse(approved)
        self.assertIn("review call failed", reason)

    def test_picks_a_model_from_the_pool_via_pick_model_fn(self):
        gap = make_gap()
        models_seen = []
        picks = []

        def tracking_call_model_fn(messages, base_url, api_key, model, *rest):
            models_seen.append(model)
            return "APPROVE"

        def tracking_pick_model_fn(models):
            picks.append(list(models))
            return models[-1]

        model_specs = [
            {"name": "model-a", "base_url": "u", "api_key": "k"},
            {"name": "model-b", "base_url": "u", "api_key": "k"},
        ]
        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "models": model_specs,
             "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=tracking_call_model_fn,
            pick_model_fn=tracking_pick_model_fn,
        )
        self.assertTrue(approved)
        self.assertEqual(models_seen, ["model-b"])
        self.assertEqual(picks, [model_specs])


class FixGapHappyPathTests(unittest.TestCase):
    def test_commits_when_build_and_tests_pass_and_gaps_shrink(self):
        gap = make_gap(gap_count=2)
        model_calls = []
        commit_calls = []

        result = fix_gap(
            gap,
            {
                "base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}],
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
    "base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}],
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

    def test_picks_a_model_from_the_pool_for_each_call_via_pick_model_fn(self):
        models_seen = []
        picks = []

        def tracking_call_model_fn(messages, base_url, api_key, model, *rest):
            models_seen.append(model)
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        def tracking_pick_model_fn(models):
            picks.append(list(models))
            return models[0]

        model_specs = [
            {"name": "model-a", "base_url": "u", "api_key": "k"},
            {"name": "model-b", "base_url": "u", "api_key": "k"},
            {"name": "model-c", "base_url": "u", "api_key": "k"},
        ]
        multi_model_config = dict(CONFIG, models=model_specs)
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=tracking_call_model_fn,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=multi_model_config,
            repo_root=Path("/fake/repo"),
            pick_model_fn=tracking_pick_model_fn,
        )

        self.assertTrue(built)
        self.assertEqual(models_seen, ["model-a"])
        self.assertEqual(picks, [model_specs])

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

    def test_nudges_model_to_submit_a_diff_once_request_budget_is_exhausted(self):
        # Previously: once request_turns_used hit MAX_REQUEST_TURNS, the
        # next REQUEST-shaped reply fell straight through to extract_diff
        # and failed immediately -- the model was never actually told to
        # stop investigating and submit something, so a whole attempt
        # could be burned on file requests with zero code ever touched.
        calls = []

        def fake_call_model(messages, *a):
            calls.append(1)
            if len(calls) <= 5:
                # Calls 1-4 consume the 4 allowed REQUEST turns; call 5 is
                # a 5th REQUEST made after the budget is already spent --
                # that's what must trigger the nudge instead of an
                # immediate silent failure.
                return "REQUEST: src/parsers/jpeg/mod.rs"
            # 6th call: this is the post-nudge turn -- submit a real diff.
            self.assertIn("No more file requests", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, max_request_turns=4),
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(calls), 6)

    def test_fails_with_specific_reason_if_model_keeps_requesting_after_the_nudge(self):
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "REQUEST: src/parsers/jpeg/mod.rs",
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=dict(CONFIG, max_request_turns=4),
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertEqual(reason, "no diff in model response (exhausted request budget)")

    def test_max_request_turns_is_configurable(self):
        calls = []

        def fake_call_model(messages, *a):
            calls.append(1)
            return "REQUEST: src/parsers/jpeg/mod.rs"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=dict(CONFIG, max_request_turns=2),
            repo_root=Path("/fake/repo"),
        )
        # 2 REQUEST turns allowed (calls 1-2), then the 3rd REQUEST triggers
        # the nudge, then the 4th (still just requesting) fails -- 4 calls
        # total, not the default cap's 22.
        self.assertEqual(len(calls), 4)
        self.assertFalse(built)

    def test_default_max_request_turns_is_twenty(self):
        calls = []

        def fake_call_model(messages, *a):
            calls.append(1)
            return "REQUEST: src/parsers/jpeg/mod.rs"

        attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,  # no max_request_turns override -- uses the default
            repo_root=Path("/fake/repo"),
        )
        # 20 REQUEST turns + 1 nudge turn + 1 final failing call = 22.
        self.assertEqual(len(calls), 22)


class FixGapFailureTests(unittest.TestCase):
    def test_fails_when_gap_count_does_not_decrease(self):
        gap = make_gap(gap_count=2)
        result = fix_gap(
            gap,
            {
                "base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}],
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
                "base_url": "u", "api_key": "k", "models": [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}],
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

    def test_review_call_model_fn_is_used_for_review_when_given_separately(self):
        # Lets a caller distinguish fixer vs reviewer calls in its own
        # logging/metrics (see model_fix_loop.py main()'s two
        # phase-tagged logging_call_model closures) -- the fixer call and
        # the review call must go to two different functions, not the
        # same shared one, when review_call_model_fn is provided.
        gap = make_gap(gap_count=2)
        fixer_calls = []
        reviewer_calls = []

        def fake_attempt_build(messages, **kwargs):
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        def fixer_call_model_fn(messages, *a):
            fixer_calls.append(messages)
            return "should not be called -- review_call_model_fn takes over review calls"

        def reviewer_call_model_fn(messages, *a):
            reviewer_calls.append(messages)
            return "APPROVE"

        result = fix_gap(
            gap, CONFIG,
            call_model_fn=fixer_call_model_fn, review_call_model_fn=reviewer_call_model_fn,
            attempt_build_fn=fake_attempt_build,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(fixer_calls), 0)
        self.assertEqual(len(reviewer_calls), 1)

    def test_review_call_model_fn_defaults_to_call_model_fn_when_absent(self):
        # Backward compatibility: existing callers that only pass
        # call_model_fn (not review_call_model_fn) must keep getting the
        # original shared-closure behavior.
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
                                    stream=False, thinking=True, temperature=0, timeout=120,
                                    max_retries=1000, retry_backoff_seconds=2, max_retry_backoff_seconds=120):
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
                                    stream=False, thinking=True, temperature=0, timeout=120,
                                    max_retries=1000, retry_backoff_seconds=2, max_retry_backoff_seconds=120):
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
                                    stream=False, thinking=True, temperature=0, timeout=120,
                                    max_retries=1000, retry_backoff_seconds=2, max_retry_backoff_seconds=120):
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

        review_config = dict(
            CONFIG,
            models=[{"name": "review-model", "base_url": "https://review.example/v1", "api_key": "k"}],
            base_url="https://review.example/v1",
        )

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
        self.assertEqual(fixer_config["models"], [{"name": "glm-5.2", "base_url": "u", "api_key": "k"}])
        self.assertEqual(
            review_seen_config["models"],
            [{"name": "review-model", "base_url": "https://review.example/v1", "api_key": "k"}],
        )
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


class FixGapDuplicateDetectionTests(unittest.TestCase):
    def _fake_attempt_build(self, messages, **kwargs):
        messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
        return True, None, "--- a/x\n+++ b/x\n", messages

    def test_detected_duplicate_short_circuits_before_calling_review(self):
        # The whole point: a detected duplicate must never reach the
        # (API-call-costing) reviewer at all -- it's rejected
        # deterministically and immediately.
        gap = make_single_tag_gap_dict(source_file=None)
        review_calls = []

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            review_fn=lambda *a, **kw: review_calls.append(1) or (True, ""),
            detect_duplicate_fn=lambda diff, tag_literal, repo_root: True,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("must not commit a detected duplicate"),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "duplicate")
        self.assertIn("APP0:OcadRevision", result["reason"])
        self.assertEqual(review_calls, [])

    def test_no_duplicate_detected_proceeds_to_normal_review(self):
        gap = make_single_tag_gap_dict(source_file=None)
        commit_calls = []

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            review_fn=lambda *a, **kw: (True, ""),
            detect_duplicate_fn=lambda diff, tag_literal, repo_root: False,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: commit_calls.append(msg),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(commit_calls), 1)

    def test_multi_tag_gap_skips_the_duplicate_check_entirely(self):
        # tag_literal_for_gap returns None for a gap with more than one
        # entry (see its own tests) -- detect_duplicate_fn must not even
        # be called in that case, not called with a meaningless literal.
        gap = make_gap(gap_count=2)
        detect_calls = []

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            review_fn=lambda *a, **kw: (True, ""),
            detect_duplicate_fn=lambda diff, tag_literal, repo_root: detect_calls.append(tag_literal) or False,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(detect_calls, [])


class RunLoopTests(unittest.TestCase):
    def test_stops_after_two_consecutive_dry_rounds(self):
        find_calls = []

        def fake_find_gaps():
            find_calls.append(1)
            return []

        result = run_loop({"models": ["x"]}, fake_find_gaps, fix_gap_fn=lambda g, c: self.fail("should not fix"))
        self.assertEqual(result["rounds"], 2)
        self.assertEqual(len(find_calls), 2)

    def test_resets_dry_streak_when_a_gap_closes(self):
        rounds = [[make_gap()], [], []]

        def fake_find_gaps():
            return rounds.pop(0)

        def fake_fix_gap(gap, config):
            return {"format": gap["format"], "status": "fixed", "gaps_closed": gap["gap_count"]}

        result = run_loop({"models": ["x"]}, fake_find_gaps, fake_fix_gap)
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

        result = run_loop({"models": ["x"]}, fake_find_gaps, fake_fix_gap)

        # NEF attempted exactly twice (rounds 1 and 2), never a third time,
        # even though round 3's fake data includes it -- proving the
        # skip-list filter in run_loop actually removes it before dispatch.
        self.assertEqual(attempts.count("NEF"), 2)
        self.assertEqual(result["skipped"], ["NEF"])

    def test_cleans_the_workspace_when_a_format_gets_skip_listed(self):
        nef_gap = make_gap()  # format "NEF"
        clean_calls = []
        rounds = [[nef_gap], [nef_gap], []]

        def fake_find_gaps():
            return rounds.pop(0) if rounds else []

        def fake_fix_gap(g, config):
            return {"format": g["format"], "status": "failed", "reason": "still broken"}

        run_loop(
            {"models": ["x"]}, fake_find_gaps, fake_fix_gap,
            git_checkout_clean_fn=lambda root: clean_calls.append(root),
            repo_root=Path("/fake/repo"),
        )

        # Cleaned exactly once, right when the 2nd failure skip-lists NEF --
        # not after the 1st failure, and not once per round thereafter.
        self.assertEqual(clean_calls, [Path("/fake/repo")])

    def test_does_not_clean_when_no_format_ever_gets_skip_listed(self):
        clean_calls = []
        rounds = [[make_gap()], []]

        def fake_find_gaps():
            return rounds.pop(0) if rounds else []

        run_loop(
            {"models": ["x"]}, fake_find_gaps,
            fix_gap_fn=lambda g, c: {"format": g["format"], "status": "fixed", "gaps_closed": g["gap_count"]},
            git_checkout_clean_fn=lambda root: clean_calls.append(root),
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(clean_calls, [])

    def test_does_not_clean_when_git_checkout_clean_fn_or_repo_root_is_omitted(self):
        rounds = [[make_gap()], [make_gap()], []]

        def fake_find_gaps():
            return rounds.pop(0) if rounds else []

        # Must not raise even though a format gets skip-listed here --
        # cleanup is opt-in, not required.
        run_loop(
            {"models": ["x"]}, fake_find_gaps,
            fix_gap_fn=lambda g, c: {"format": g["format"], "status": "failed", "reason": "still broken"},
        )


class TagKeyForTests(unittest.TestCase):
    def test_missing_tag_uses_family_and_name(self):
        entry = {"family": "EXIF", "name": "LensModel"}
        self.assertEqual(tag_key_for("NEF", entry, "missing"), "NEF:EXIF:LensModel")

    def test_diff_tag_uses_existing_tag_key(self):
        entry = {"tag_key": "EXIF:ISO"}
        self.assertEqual(tag_key_for("NEF", entry, "diff"), "NEF:EXIF:ISO")


class ExpandGapsToTagsTests(unittest.TestCase):
    def test_flattens_missing_and_diff_entries_across_formats(self):
        gaps = [
            make_gap(),  # format NEF: 1 missing_tags entry, 1 value_differences entry
            {
                "format": "PNG",
                "missing_tags": [{"family": "PNG", "name": "Gamma", "value": "1", "tag_id": None, "source_file": None}],
                "value_differences": [],
                "gap_count": 1,
                "parser_files": ["src/parsers/png/mod.rs"],
            },
        ]
        tag_gaps = expand_gaps_to_tags(gaps)
        self.assertEqual(len(tag_gaps), 3)
        keys = {tg["tag_key"] for tg in tag_gaps}
        self.assertEqual(keys, {"NEF:EXIF:LensModel", "NEF:EXIF:ISO", "PNG:PNG:Gamma"})
        kinds = {tg["tag_key"]: tg["kind"] for tg in tag_gaps}
        self.assertEqual(kinds["NEF:EXIF:LensModel"], "missing")
        self.assertEqual(kinds["NEF:EXIF:ISO"], "diff")

    def test_empty_gaps_list_yields_no_tags(self):
        self.assertEqual(expand_gaps_to_tags([]), [])


class MakeSingleTagGapTests(unittest.TestCase):
    def test_missing_kind_populates_missing_tags_only(self):
        tag_gap = {
            "format": "NEF", "tag_key": "NEF:EXIF:LensModel", "kind": "missing",
            "entry": {"family": "EXIF", "name": "LensModel"}, "parser_files": ["a.rs"],
        }
        gap = make_single_tag_gap(tag_gap)
        self.assertEqual(gap["missing_tags"], [tag_gap["entry"]])
        self.assertEqual(gap["value_differences"], [])
        self.assertEqual(gap["gap_count"], 1)
        self.assertEqual(gap["format"], "NEF")
        self.assertEqual(gap["parser_files"], ["a.rs"])

    def test_diff_kind_populates_value_differences_only(self):
        tag_gap = {
            "format": "NEF", "tag_key": "NEF:EXIF:ISO", "kind": "diff",
            "entry": {"tag_key": "EXIF:ISO"}, "parser_files": [],
        }
        gap = make_single_tag_gap(tag_gap)
        self.assertEqual(gap["missing_tags"], [])
        self.assertEqual(gap["value_differences"], [tag_gap["entry"]])
        self.assertEqual(gap["gap_count"], 1)


class RunTagLoopTests(unittest.TestCase):
    def _state_io(self):
        store = {}

        def load(_path):
            # A real on-disk load (json.loads) always produces fresh
            # objects with no shared references back to what was last
            # saved -- a shallow dict(store) here would let two "loads"
            # share the same nested list/dict objects, which a save
            # in between then mutates retroactively under both callers'
            # feet. json round-trip is a simple, correct deep copy.
            return json.loads(json.dumps(store))

        def save(_path, state):
            store.clear()
            store.update(json.loads(json.dumps(state)))

        return store, load, save

    def test_stops_when_no_tags_remain(self):
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: [], fix_gap_fn=lambda *a: self.fail("should not fix"),
            state_path="/fake/state.json",
            load_state_fn=lambda p: {}, save_state_fn=lambda p, s: None,
        )
        self.assertEqual(result["rounds"], 1)
        self.assertEqual(result["fixed"], [])

    def test_attempts_exactly_one_tag_per_round(self):
        gaps = [make_gap()]  # 2 tags: NEF:EXIF:LensModel (missing), NEF:EXIF:ISO (diff)
        attempts = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(tag_gap["tag_key"])
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1,
        )
        # Exactly one tag attempted this round, not both -- one tag per
        # loop process/round, per the "limit it down to 1 tag" design.
        self.assertEqual(len(attempts), 1)

    def test_blacklists_a_tag_after_two_failures_not_the_whole_format(self):
        gaps = [make_gap()]  # NEF:EXIF:LensModel (missing) picked first each round
        attempts = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(tag_gap["tag_key"])
            if tag_gap["tag_key"] == "NEF:EXIF:LensModel":
                return {"status": "failed", "reason": "nope"}
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=2, max_fails=2,
        )
        self.assertEqual(attempts, ["NEF:EXIF:LensModel", "NEF:EXIF:LensModel"])
        self.assertTrue(store["NEF:EXIF:LensModel"]["blacklisted"])
        self.assertEqual(store["NEF:EXIF:LensModel"]["fails"], 2)

    def test_blacklisting_records_when_and_by_which_worker(self):
        # A dashboard reading tag-state.json needs to answer "when was
        # this blacklisted" and "which worker gave up on it" without
        # relying on that worker's own log -- which gets truncated on
        # every respawn, so it can't be trusted to still hold this
        # history by the time anyone looks.
        gaps = [make_gap()]

        def fake_fix(tag_gap, config, previous_attempts=None):
            if tag_gap["tag_key"] == "NEF:EXIF:LensModel":
                return {"status": "failed", "reason": "nope"}
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        before = time.time()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=2, max_fails=2, worker_id="3",
        )
        after = time.time()
        entry = store["NEF:EXIF:LensModel"]
        self.assertTrue(entry["blacklisted"])
        self.assertEqual(entry["blacklisted_by"], "3")
        self.assertGreaterEqual(entry["blacklisted_at"], before)
        self.assertLessEqual(entry["blacklisted_at"], after)

    def test_default_max_fails_is_ten(self):
        gaps = [make_gap()]
        attempts = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(1)
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=9,
        )
        # 9 failures on the same tag (LensModel picked every round, since
        # the diff-kind ISO tag is never blacklisted or exhausted) must
        # NOT blacklist it yet under the new default of 10.
        self.assertFalse(store["NEF:EXIF:LensModel"]["blacklisted"])
        self.assertEqual(store["NEF:EXIF:LensModel"]["fails"], 9)

    def test_previous_attempts_carried_forward_and_history_recorded(self):
        gaps = [make_gap()]
        seen_history = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            if tag_gap["tag_key"] == "NEF:EXIF:LensModel":
                seen_history.append(previous_attempts)
                return {"status": "failed", "reason": f"attempt {len(seen_history)} failed", "diff": f"diff-{len(seen_history)}"}
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=3, max_fails=10,
        )
        # Round 1 sees no history yet; round 2 sees round 1's; round 3
        # sees both -- context accumulates round over round for this tag.
        self.assertEqual(seen_history[0], [])
        self.assertEqual(len(seen_history[1]), 1)
        self.assertEqual(seen_history[1][0]["diff"], "diff-1")
        self.assertEqual(seen_history[1][0]["reason"], "attempt 1 failed")
        self.assertEqual(len(seen_history[2]), 2)
        self.assertEqual(seen_history[2][1]["diff"], "diff-2")

    def test_blacklist_full_stops_instead_of_resetting(self):
        gaps = [make_gap()]

        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        store["NEF:EXIF:LensModel"] = {"fails": 10, "blacklisted": True}
        store["NEF:EXIF:ISO"] = {"fails": 10, "blacklisted": True}
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=5, blacklist_full=True,
        )
        # Must stop immediately (round 1) rather than reset-and-continue.
        self.assertEqual(result["rounds"], 1)
        self.assertEqual(result["cycles_reset"], 0)

    def test_max_distinct_tags_stops_onboarding_new_tags(self):
        # 3 distinct tags across two formats; cap this process at 1.
        gaps = [
            make_gap(),  # NEF: LensModel (missing), ISO (diff)
            {
                "format": "PNG",
                "missing_tags": [{"family": "PNG", "name": "Gamma", "value": "1", "tag_id": None, "source_file": None}],
                "value_differences": [], "gap_count": 1, "parser_files": [],
            },
        ]
        attempts = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(tag_gap["tag_key"])
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=5, max_fails=10, max_distinct_tags=1,
        )
        # Only the first tag ever picked (NEF:EXIF:LensModel) gets
        # attempted, repeatedly -- the loop must stop rather than start
        # PNG:PNG:Gamma or NEF:EXIF:ISO once the cap of 1 distinct tag is
        # reached.
        self.assertEqual(set(attempts), {"NEF:EXIF:LensModel"})
        self.assertEqual(result["distinct_tags_seen"], 1)

    def test_worker_claim_prevents_another_worker_from_picking_same_tag(self):
        gaps = [make_gap()]
        attempts_by_worker = {"a": [], "b": []}

        store, load, save = self._state_io()
        # Simulate worker "a" having already claimed LensModel recently.
        store["NEF:EXIF:LensModel"] = {
            "fails": 0, "blacklisted": False, "attempts": [],
            "claimed_by": "a", "claimed_at": time.time(),
        }

        def fake_fix_b(tag_gap, config, previous_attempts=None):
            attempts_by_worker["b"].append(tag_gap["tag_key"])
            return {"status": "failed", "reason": "nope"}

        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix_b,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, worker_id="b",
        )
        # Worker "b" must pick the OTHER tag (ISO), not the one "a" holds.
        self.assertEqual(attempts_by_worker["b"], ["NEF:EXIF:ISO"])

    def test_stale_claim_can_be_reclaimed(self):
        gaps = [make_gap()]
        attempts = []

        store, load, save = self._state_io()
        # Claimed a long time ago -- treated as an abandoned/crashed worker.
        store["NEF:EXIF:LensModel"] = {
            "fails": 0, "blacklisted": False, "attempts": [],
            "claimed_by": "a", "claimed_at": time.time() - 999999,
        }

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(tag_gap["tag_key"])
            return {"status": "failed", "reason": "nope"}

        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, worker_id="b", claim_stale_seconds=1800,
        )
        self.assertIn("NEF:EXIF:LensModel", attempts)

    def test_blacklisted_tag_is_skipped_in_favor_of_another(self):
        gaps = [make_gap()]  # LensModel (missing) + ISO (diff)
        attempts = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(tag_gap["tag_key"])
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        store["NEF:EXIF:LensModel"] = {"fails": 2, "blacklisted": True}
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1,
        )
        # LensModel is blacklisted -- must never be attempted; ISO (the
        # other tag in this format) gets picked instead.
        self.assertEqual(attempts, ["NEF:EXIF:ISO"])

    def test_resets_blacklist_once_every_remaining_tag_is_blacklisted(self):
        gaps = [make_gap()]  # both LensModel and ISO already blacklisted
        attempts = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(tag_gap["tag_key"])
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        store["NEF:EXIF:LensModel"] = {"fails": 2, "blacklisted": True}
        store["NEF:EXIF:ISO"] = {"fails": 2, "blacklisted": True}
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=2,
        )
        # Round 1: everything blacklisted -> reset (no attempt made).
        # Round 2: blacklist is empty again -> one of the two tags gets a
        # fresh attempt.
        self.assertEqual(result["cycles_reset"], 1)
        self.assertEqual(len(attempts), 1)

    def test_fixed_tag_clears_its_state_entry(self):
        gaps = [make_gap()]

        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        store["NEF:EXIF:LensModel"] = {"fails": 1, "blacklisted": False}
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1,
        )
        self.assertNotIn("NEF:EXIF:LensModel", store)

    def test_persists_state_via_save_state_fn(self):
        gaps = [make_gap()]
        written = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "failed", "reason": "nope"}

        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json",
            load_state_fn=lambda p: {},
            save_state_fn=lambda p, s: written.append((p, dict(s))),
            max_rounds=1,
        )
        self.assertEqual(written[-1][0], "/fake/state.json")
        self.assertIn("NEF:EXIF:LensModel", written[-1][1])

    def test_calls_git_checkout_clean_only_when_a_tag_gets_blacklisted(self):
        gaps = [make_gap()]
        clean_calls = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            git_checkout_clean_fn=lambda root: clean_calls.append(root),
            repo_root=Path("/fake/repo"),
            max_rounds=1, max_fails=2,
        )
        # First failure only -- not blacklisted yet, so no cleanup call.
        self.assertEqual(clean_calls, [])

        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            git_checkout_clean_fn=lambda root: clean_calls.append(root),
            repo_root=Path("/fake/repo"),
            max_rounds=1, max_fails=2,
        )
        # Second failure -- now blacklisted, cleanup must fire.
        self.assertEqual(clean_calls, [Path("/fake/repo")])

    def test_duplicate_status_is_skipped_not_failed_and_not_blacklisted(self):
        # A tag another worker already fixed elsewhere (see fix_gap's
        # detect_duplicate_fn) must never count against this tag's fail
        # budget -- it isn't this tag's fault someone else got there
        # first. Confirmed with max_fails=1: if "duplicate" were treated
        # as a failure, a single one would immediately blacklist it.
        gaps = [make_gap()]

        def fake_fix(tag_gap, config, previous_attempts=None):
            if tag_gap["tag_key"] == "NEF:EXIF:LensModel":
                return {"status": "duplicate", "reason": "already fixed elsewhere"}
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, max_fails=1,
        )
        self.assertEqual(len(result["skipped"]), 1)
        self.assertEqual(result["skipped"][0]["tag_key"], "NEF:EXIF:LensModel")
        self.assertEqual(result["failed"], [])
        # Popped from state entirely, same cleanup as a genuine fix --
        # not left sitting around with a fail count or blacklist flag.
        self.assertNotIn("NEF:EXIF:LensModel", store)

    def test_refresh_worktree_fn_is_called_once_per_round(self):
        gaps = [make_gap()]
        refresh_calls = []

        def fake_refresh():
            refresh_calls.append(1)
            return True, "ok"

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps,
            fix_gap_fn=lambda tg, c, previous_attempts=None: {"status": "failed", "reason": "nope"},
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=3, refresh_worktree_fn=fake_refresh,
        )
        self.assertEqual(len(refresh_calls), 3)

    def test_no_refresh_worktree_fn_given_does_not_crash(self):
        # Default (refresh_worktree_fn=None) -- standalone runs with no
        # shared branch to refresh against must work exactly as before.
        gaps = [make_gap()]
        store, load, save = self._state_io()
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps,
            fix_gap_fn=lambda tg, c, previous_attempts=None: {"status": "failed", "reason": "nope"},
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1,
        )
        self.assertEqual(result["rounds"], 1)

    def test_failed_refresh_is_logged_but_does_not_stop_the_round(self):
        gaps = [make_gap()]
        logged = []

        store, load, save = self._state_io()
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps,
            fix_gap_fn=lambda tg, c, previous_attempts=None: {"status": "failed", "reason": "nope"},
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, refresh_worktree_fn=lambda: (False, "not possible to fast-forward"),
            log_fn=logged.append,
        )
        self.assertEqual(result["rounds"], 1)
        self.assertTrue(any("refresh skipped" in line for line in logged))


if __name__ == "__main__":
    unittest.main()
