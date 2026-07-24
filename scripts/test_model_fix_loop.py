import json
import os
import tempfile
import time
import unittest
import urllib.error
from unittest.mock import patch, MagicMock
from pathlib import Path

from model_fix_loop import (
    ARCHITECTURE_PRIMER,
    KNOWN_PITFALLS,
    _normalize_model_config,
    attempt_build,
    build_exact_sample_block,
    build_failure_critique_prompt,
    build_format_overview_block,
    build_neighbor_precedent_block,
    build_perl_reference_block,
    build_prompt,
    build_reply_shape_manifest,
    build_review_prompt,
    critique_failed_attempt,
    cargo_build,
    cargo_check,
    cargo_env,
    cargo_test_targeted,
    cargo_test_workspace,
    apply_prompt_cache_markers,
    call_model,
    extract_cache_usage,
    cluster_key,
    compact_messages,
    DEFAULT_GOVERNOR_BURST,
    detect_duplicate_tag_insertion,
    estimate_tokens,
    expand_gaps_to_tags,
    extract_diff,
    append_format_memory_note,
    extract_perl_table_notes,
    extract_perl_tag_snippet,
    extract_review_verdict,
    file_content_at_head,
    find_implemented_sibling,
    fix_gap,
    format_previous_attempts,
    load_format_memory,
    summarize_format_memory,
    format_sweep_review_history,
    git_apply,
    git_checkout_clean,
    git_commit,
    governor_acquire,
    governor_report,
    load_landed_tags,
    load_recent_sweep_reviews,
    load_toml_config,
    make_cluster_gap,
    make_single_tag_gap,
    models_for_phase,
    parse_request_range,
    refresh_worktree,
    resolve_request,
    review_verdict,
    RUST_ARCHITECTURE_CONSTRAINTS,
    run_loop,
    run_tag_loop,
    tag_key_for,
    tag_literal_for_gap,
    tag_still_open,
    TERMINAL_REMINDER,
    truncate_to_token_budget,
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
        self.assertEqual(config["stream"], True)  # streaming on by default
        self.assertEqual(config["prompt_cache"], "auto")
        self.assertEqual(config["thinking"], True)
        self.assertEqual(config["temperature"], 0)

    def test_preserves_explicit_values(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m1", "m2"],
            "max_tokens": 16, "reasoning_effort": "low", "stream": True,
            "thinking": False, "temperature": 0.7,
        })
        self.assertEqual(config["models"], [
            {"name": "m1", "base_url": "u", "api_key": "k", "phase": None, "reasoning_effort": None},
            {"name": "m2", "base_url": "u", "api_key": "k", "phase": None, "reasoning_effort": None},
        ])
        self.assertEqual(config["max_tokens"], 16)
        self.assertEqual(config["stream"], True)

    def test_string_model_entries_inherit_table_base_url_and_api_key(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["models"], [
            {"name": "m", "base_url": "u", "api_key": "k", "phase": None, "reasoning_effort": None},
        ])

    def test_table_model_entries_can_override_base_url_and_api_key(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k",
            "models": [
                "shared-provider-model",
                {"name": "other-provider-model", "base_url": "https://other.example/v1", "api_key": "other-key"},
            ],
        })
        self.assertEqual(config["models"], [
            {"name": "shared-provider-model", "base_url": "u", "api_key": "k",
             "phase": None, "reasoning_effort": None},
            {"name": "other-provider-model", "base_url": "https://other.example/v1", "api_key": "other-key",
             "phase": None, "reasoning_effort": None},
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

    def test_new_harness_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["max_request_repeats"], 3)
        self.assertEqual(config["max_verify_turns"], 10)
        self.assertEqual(config["compaction_trigger_tokens"], 12_000)
        self.assertEqual(config["compaction_keep_recent_turns"], 4)

    def test_new_harness_knobs_are_overridable(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m"],
            "max_request_repeats": 5, "max_verify_turns": 2,
            "compaction_trigger_tokens": 6000, "compaction_keep_recent_turns": 8,
        })
        self.assertEqual(config["max_request_repeats"], 5)
        self.assertEqual(config["max_verify_turns"], 2)
        self.assertEqual(config["compaction_trigger_tokens"], 6000)
        self.assertEqual(config["compaction_keep_recent_turns"], 8)

    def test_governor_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["governor_calls_per_minute"], 30)
        self.assertEqual(config["governor_burst"], 5)
        self.assertEqual(config["governor_cooldown_seconds"], 30)
        self.assertEqual(config["governor_max_cooldown_seconds"], 300)

    def test_throughput_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["max_cluster_tags"], 6)
        self.assertEqual(config["use_sccache"], True)
        self.assertEqual(config["governor_calls_per_minute"], 30)


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


class EstimateTokensTests(unittest.TestCase):
    def test_roughly_four_chars_per_token(self):
        self.assertEqual(estimate_tokens("a" * 400), 100)

    def test_never_returns_zero_for_nonempty_text(self):
        self.assertEqual(estimate_tokens("hi"), 1)


class TruncateToTokenBudgetTests(unittest.TestCase):
    def test_leaves_short_text_untouched(self):
        text = "short prompt"
        self.assertEqual(truncate_to_token_budget(text, max_tokens=100), text)

    def test_truncates_long_text_and_appends_marker(self):
        text = "x" * 100_000
        result = truncate_to_token_budget(text, max_tokens=10)
        self.assertLess(len(result), len(text))
        self.assertTrue(result.startswith("x" * 40))
        self.assertIn("truncated to fit the ~10-token budget", result)

    def test_truncation_keeps_the_start_of_the_text(self):
        text = "KEEP_THIS_PREFIX" + "z" * 100_000
        result = truncate_to_token_budget(text, max_tokens=10)
        self.assertTrue(result.startswith("KEEP_THIS_PREFIX"))


class CompactMessagesTests(unittest.TestCase):
    def _messages(self):
        big = "x" * 8000   # ~2000 estimated tokens
        return [
            {"role": "user", "content": "initial prompt " + "p" * 8000},
            {"role": "assistant", "content": "REQUEST: src/a.rs"},
            {"role": "user", "content": "Contents of src/a.rs:\n" + big},
            {"role": "assistant", "content": "REQUEST: src/b.rs"},
            {"role": "user", "content": "Contents of src/b.rs:\n" + big},
            {"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```"},
            {"role": "user", "content": "That diff did not apply: whitespace\nPlease resend a corrected diff."},
        ]

    def test_below_trigger_returns_messages_unchanged(self):
        messages = self._messages()
        result = compact_messages(messages, trigger_tokens=10_000_000)
        self.assertEqual(result, messages)

    def test_above_trigger_stubs_old_large_user_turns_only(self):
        messages = self._messages()
        result = compact_messages(messages, trigger_tokens=100, keep_recent=2)
        # message 0 (initial prompt) is never touched
        self.assertEqual(result[0], messages[0])
        # assistant turns are never touched
        self.assertEqual(result[1], messages[1])
        self.assertEqual(result[3], messages[3])
        self.assertEqual(result[5], messages[5])
        # old large served payloads are stubbed
        self.assertIn("[earlier content elided for space:", result[2]["content"])
        self.assertIn("Contents of src/a.rs:", result[2]["content"])  # first line kept
        # the last keep_recent=2 messages are untouched
        self.assertEqual(result[5], messages[5])
        self.assertEqual(result[6], messages[6])

    def test_small_user_turns_are_not_stubbed(self):
        messages = self._messages()
        result = compact_messages(messages, trigger_tokens=100, keep_recent=0)
        # the small repair prompt (message 6) is under the elide floor
        self.assertEqual(result[6], messages[6])

    def test_compaction_is_idempotent(self):
        messages = self._messages()
        once = compact_messages(messages, trigger_tokens=100, keep_recent=2)
        twice = compact_messages(once, trigger_tokens=100, keep_recent=2)
        self.assertEqual(once, twice)

    def test_original_list_is_not_mutated(self):
        messages = self._messages()
        snapshot = [dict(m) for m in messages]
        compact_messages(messages, trigger_tokens=100, keep_recent=2)
        self.assertEqual(messages, snapshot)


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


class ExtractCacheUsageTests(unittest.TestCase):
    def test_openai_shape(self):
        usage = {"prompt_tokens": 5000, "prompt_tokens_details": {"cached_tokens": 4000}}
        self.assertEqual(extract_cache_usage(usage), (4000, 5000))

    def test_anthropic_shape(self):
        usage = {"input_tokens": 5000, "cache_read_input_tokens": 4000}
        self.assertEqual(extract_cache_usage(usage), (4000, 5000))

    def test_openai_zero_cache_hits_is_reported_not_dropped(self):
        usage = {"prompt_tokens": 5000, "prompt_tokens_details": {"cached_tokens": 0}}
        self.assertEqual(extract_cache_usage(usage), (0, 5000))

    def test_none_when_no_cache_info(self):
        self.assertIsNone(extract_cache_usage(None))
        self.assertIsNone(extract_cache_usage({}))
        self.assertIsNone(extract_cache_usage({"prompt_tokens": 5000}))  # no details/cache field


class ApplyPromptCacheMarkersTests(unittest.TestCase):
    def test_explicit_wraps_first_message_in_cache_control_block(self):
        messages = [{"role": "user", "content": "big static prompt"},
                    {"role": "assistant", "content": "reply"}]
        out = apply_prompt_cache_markers(messages, "explicit")
        self.assertEqual(out[0]["content"], [
            {"type": "text", "text": "big static prompt", "cache_control": {"type": "ephemeral"}}
        ])
        self.assertEqual(out[1], {"role": "assistant", "content": "reply"})  # rest untouched

    def test_explicit_does_not_mutate_input(self):
        messages = [{"role": "user", "content": "X"}]
        apply_prompt_cache_markers(messages, "explicit")
        self.assertEqual(messages, [{"role": "user", "content": "X"}])  # original unchanged

    def test_auto_and_off_are_identity(self):
        messages = [{"role": "user", "content": "X"}]
        self.assertEqual(apply_prompt_cache_markers(messages, "auto"), messages)
        self.assertEqual(apply_prompt_cache_markers(messages, "off"), messages)

    def test_explicit_leaves_non_string_content_alone(self):
        messages = [{"role": "user", "content": [{"type": "text", "text": "X"}]}]
        self.assertEqual(apply_prompt_cache_markers(messages, "explicit"), messages)

    def test_explicit_handles_empty_messages(self):
        self.assertEqual(apply_prompt_cache_markers([], "explicit"), [])


class CallModelCachingTests(unittest.TestCase):
    def _mock_json_response(self, mock_urlopen, payload):
        mock_cm = MagicMock()
        mock_cm.read.return_value = json.dumps(payload).encode()
        mock_urlopen.return_value.__enter__.return_value = mock_cm

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_usage_fn_receives_usage_object(self, mock_urlopen):
        usage = {"prompt_tokens": 100, "prompt_tokens_details": {"cached_tokens": 80}}
        self._mock_json_response(mock_urlopen, {
            "choices": [{"message": {"content": "diff"}}], "usage": usage,
        })
        captured = []
        result = call_model(
            [{"role": "user", "content": "fix it"}], base_url="https://api.test/v1", api_key="k",
            model="m", max_tokens=10, reasoning_effort="max",
            usage_fn=captured.append,
        )
        self.assertEqual(result, "diff")
        self.assertEqual(captured, [usage])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_usage_fn_receives_none_when_provider_omits_usage(self, mock_urlopen):
        self._mock_json_response(mock_urlopen, {"choices": [{"message": {"content": "diff"}}]})
        captured = []
        call_model(
            [{"role": "user", "content": "x"}], base_url="https://api.test/v1", api_key="k",
            model="m", max_tokens=10, reasoning_effort="max", usage_fn=captured.append,
        )
        self.assertEqual(captured, [None])

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_explicit_prompt_cache_sends_cache_control_block(self, mock_urlopen):
        self._mock_json_response(mock_urlopen, {"choices": [{"message": {"content": "d"}}]})
        call_model(
            [{"role": "user", "content": "static prefix"}], base_url="https://api.test/v1", api_key="k",
            model="m", max_tokens=10, reasoning_effort="max", prompt_cache="explicit",
        )
        body = json.loads(mock_urlopen.call_args[0][0].data)
        self.assertEqual(body["messages"][0]["content"][0]["cache_control"], {"type": "ephemeral"})

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_auto_prompt_cache_sends_plain_string_content(self, mock_urlopen):
        self._mock_json_response(mock_urlopen, {"choices": [{"message": {"content": "d"}}]})
        call_model(
            [{"role": "user", "content": "static prefix"}], base_url="https://api.test/v1", api_key="k",
            model="m", max_tokens=10, reasoning_effort="max", prompt_cache="auto",
        )
        body = json.loads(mock_urlopen.call_args[0][0].data)
        self.assertEqual(body["messages"][0]["content"], "static prefix")
        self.assertNotIn("stream_options", body)  # non-streaming

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_streaming_adds_stream_options_and_captures_usage(self, mock_urlopen):
        usage = {"prompt_tokens": 200, "prompt_tokens_details": {"cached_tokens": 150}}
        lines = [
            b'data: {"choices": [{"delta": {"content": "hello "}}]}\n',
            b'data: {"choices": [{"delta": {"content": "world"}}]}\n',
            (b'data: {"choices": [], "usage": ' + json.dumps(usage).encode() + b'}\n'),
            b'data: [DONE]\n',
        ]
        mock_cm = MagicMock()
        mock_cm.__iter__.return_value = iter(lines)
        mock_urlopen.return_value.__enter__.return_value = mock_cm
        captured = []
        result = call_model(
            [{"role": "user", "content": "x"}], base_url="https://api.test/v1", api_key="k", model="m",
            max_tokens=10, reasoning_effort="max", stream=True, usage_fn=captured.append,
        )
        self.assertEqual(result, "hello world")
        self.assertEqual(captured, [usage])
        body = json.loads(mock_urlopen.call_args[0][0].data)
        self.assertEqual(body["stream_options"], {"include_usage": True})


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

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_429_is_retried(self, mock_urlopen):
        ok_body = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_cm = MagicMock()
        ok_cm.read.return_value = ok_body
        ok_response = MagicMock()
        ok_response.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [self._http_error(429), ok_response]
        reply = call_model(
            [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "max",
            sleep_fn=lambda s: None,
        )
        self.assertEqual(reply, "hi")

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_governor_is_acquired_per_attempt_and_reported(self, mock_urlopen):
        ok_body = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_cm = MagicMock()
        ok_cm.read.return_value = ok_body
        ok_response = MagicMock()
        ok_response.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [self._http_error(429), ok_response]
        with tempfile.TemporaryDirectory() as tmpdir:
            gov = Path(tmpdir) / "gov.json"
            # cooldown_seconds=0: the 429 must still be REPORTED (streak
            # increments, then the success resets it) without creating a
            # real-wall-clock cooldown this test would have to sit out --
            # cooldown waiting itself is covered by RateGovernorTests with
            # injected clocks.
            call_model(
                [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "max",
                sleep_fn=lambda s: None, governor_path=gov,
                governor_cooldown_seconds=0, governor_max_cooldown_seconds=0,
            )
            state = json.loads(gov.read_text())
        # limited once (the 429) then reset by the success
        self.assertEqual(state["consecutive_limited"], 0)
        self.assertLess(state["tokens"], DEFAULT_GOVERNOR_BURST)  # slots were spent


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
        mock_run.return_value = MagicMock(returncode=0, stdout="all good", stderr="")
        success, output = cargo_test_workspace(Path("/fake/repo"))
        self.assertTrue(success)
        self.assertEqual(output, "all good")

    @patch("model_fix_loop.subprocess.run")
    def test_false_on_nonzero_exit_includes_combined_output(self, mock_run):
        mock_run.return_value = MagicMock(
            returncode=1, stdout="test result: FAILED. 1 failed", stderr="thread panicked",
        )
        success, output = cargo_test_workspace(Path("/fake/repo"))
        self.assertFalse(success)
        self.assertIn("test result: FAILED", output)
        self.assertIn("thread panicked", output)


class CargoTestTargetedTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_runs_lib_tests_with_the_filter(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="ok\n", stderr="")
        ok, output = cargo_test_targeted(Path("/fake"), "app12")
        self.assertTrue(ok)
        self.assertEqual(mock_run.call_args[0][0], ["cargo", "test", "--lib", "app12"])


class CargoEnvSccacheTests(unittest.TestCase):
    @patch("model_fix_loop.shutil.which")
    def test_sets_wrapper_when_available_and_enabled(self, mock_which):
        mock_which.return_value = "/opt/homebrew/bin/sccache"
        with patch.dict(os.environ, {"OXIDEX_USE_SCCACHE": "1"}, clear=False):
            os.environ.pop("RUSTC_WRAPPER", None)
            env = cargo_env()
        self.assertEqual(env.get("RUSTC_WRAPPER"), "sccache")

    @patch("model_fix_loop.shutil.which")
    def test_disabled_by_env_flag(self, mock_which):
        mock_which.return_value = "/opt/homebrew/bin/sccache"
        with patch.dict(os.environ, {"OXIDEX_USE_SCCACHE": "0"}, clear=False):
            env = cargo_env()
        self.assertNotEqual(env.get("RUSTC_WRAPPER"), "sccache")


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

    def test_always_includes_known_pitfalls(self):
        prompt = build_prompt(make_gap(gap_count=1))
        self.assertIn(KNOWN_PITFALLS, prompt)

    def test_omits_perl_reference_section_when_lib_dir_not_given(self):
        prompt = build_prompt(make_gap(gap_count=1))
        self.assertNotIn("ExifTool's own Perl source", prompt)

    def test_includes_perl_reference_section_when_lib_dir_given(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            lib_dir = Path(tmpdir)
            (lib_dir / "Exif.pm").write_text(EXIFTOOL_PM_FIXTURE)
            gap = {
                "format": "NEF",
                "missing_tags": [
                    {"family": "EXIF", "name": "CFAPattern2", "value": "2 1 1 0",
                     "tag_id": None, "source_file": None},
                ],
                "value_differences": [],
                "gap_count": 1,
                "parser_files": [],
            }
            prompt = build_prompt(gap, perl_lib_dir=lib_dir)
        self.assertIn("ExifTool's own Perl source", prompt)
        self.assertIn("CFAPattern2", prompt)
        self.assertIn("Format => 'int8u'", prompt)

    def test_omits_sweep_review_section_when_log_path_not_given(self):
        prompt = build_prompt(make_gap(gap_count=1))
        self.assertNotIn("Recent sweep-review outcomes", prompt)

    def test_includes_sweep_review_section_when_relevant_entries_exist(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "sweep-review-history.jsonl"
            log_path.write_text(
                json.dumps({
                    "timestamp": "2026-07-23T10:00:00", "format": "NEF",
                    "tag": "ExifIFD:CFAPattern", "verdict": "rejected",
                    "reason": "hardcoded EXIF: prefix instead of using lookup_tag_name",
                    "commit": "abc123",
                }) + "\n"
            )
            gap = make_gap(gap_count=2)  # format NEF, per make_gap's own fixture
            prompt = build_prompt(gap, sweep_review_log_path=log_path)
        self.assertIn("Recent sweep-review outcomes", prompt)
        self.assertIn("REJECTED ExifIFD:CFAPattern", prompt)
        self.assertIn("hardcoded EXIF: prefix", prompt)

    def test_omits_sweep_review_section_when_no_entries_for_this_format(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "sweep-review-history.jsonl"
            log_path.write_text(
                json.dumps({
                    "timestamp": "2026-07-23T10:00:00", "format": "JPEG",
                    "tag": "APP12:JPEG1", "verdict": "accepted", "reason": "matches ExifTool",
                    "commit": None,
                }) + "\n"
            )
            prompt = build_prompt(make_gap(gap_count=2), sweep_review_log_path=log_path)
        self.assertNotIn("Recent sweep-review outcomes", prompt)


class BuildPromptTokenBudgetTests(unittest.TestCase):
    def test_always_explains_the_patch_chunking_protocol(self):
        prompt = build_prompt(make_gap(gap_count=2))
        self.assertIn("PATCH 1/N", prompt)
        self.assertIn("```diff", prompt)

    def test_mentions_the_configured_token_budget(self):
        prompt = build_prompt(make_gap(gap_count=2), max_prompt_tokens=4096)
        self.assertIn("roughly 4096 tokens", prompt)

    def test_default_prompt_fits_within_the_default_token_budget(self):
        gap = make_gap(gap_count=2)
        prompt = build_prompt(gap)
        self.assertLessEqual(estimate_tokens(prompt), 4096)

    def test_a_tiny_token_budget_truncates_the_prompt(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "big.rs").write_text("x" * 100_000)
            gap = {
                "format": "JPEG", "missing_tags": [], "value_differences": [],
                "gap_count": 0, "parser_files": ["big.rs"],
            }
            prompt = build_prompt(
                gap, repo_root=tmp, max_tags=40, max_file_bytes=200_000, max_prompt_tokens=50,
            )
        self.assertIn("truncated to fit the ~50-token budget", prompt)
        # Far short of what the untruncated prompt (100,000-char file alone) would be --
        # the exact length includes the marker text's own overhead, so this checks
        # order-of-magnitude truncation happened rather than an exact byte count.
        self.assertLess(len(prompt), 1000)


class BuildPromptOrderingTests(unittest.TestCase):
    def test_static_sections_precede_gap_content(self):
        prompt = build_prompt(make_gap(gap_count=2))
        gap_pos = prompt.index("Missing entirely")
        self.assertLess(prompt.index("CRITICAL RUST ARCHITECTURE CONSTRAINTS"), gap_pos)
        self.assertLess(prompt.index("Lessons from mistakes"), gap_pos)
        self.assertLess(prompt.index("exactly one of these four shapes"), gap_pos)

    def test_volatile_history_comes_after_gap_content(self):
        attempts = [{"diff": "--- a/x\n", "status": "failed", "reason": "build failed"}]
        prompt = build_prompt(make_gap(gap_count=2), previous_attempts=attempts)
        self.assertGreater(
            prompt.index("Previous attempts on this exact tag"),
            prompt.index("Missing entirely"),
        )

    def test_terminal_reminder_is_the_last_line(self):
        prompt = build_prompt(make_gap(gap_count=2))
        self.assertTrue(prompt.rstrip().endswith(TERMINAL_REMINDER.rstrip()))

    def test_manifest_lists_all_four_shapes_and_range_syntax(self):
        manifest = build_reply_shape_manifest(4096)
        for needle in ("REQUEST:", "VERIFY", "PATCH 1/N", "Plan + diff",
                       ":<start>-<end>", "roughly 4096 tokens", "ephemeral"):
            self.assertIn(needle, manifest)


class RustArchitectureConstraintsTests(unittest.TestCase):
    def test_block_contains_the_six_core_directives(self):
        for needle in (
            "Box<dyn Any>",            # no dynamic-typing crutches
            "regex",                   # no regex on binary
            "self-referential",        # no self-referential IFD structs
            "lookup_tag_name()",       # no inlined lookup tables
            "global mutable state",    # no new globals
            "unwrap()",                # no unwrap/panic on parsed data
        ):
            self.assertIn(needle, RUST_ARCHITECTURE_CONSTRAINTS)

    def test_block_contains_endianness_and_builtin_map_bullets(self):
        self.assertIn("function signatures", RUST_ARCHITECTURE_CONSTRAINTS)
        self.assertIn("u32::from_be_bytes", RUST_ARCHITECTURE_CONSTRAINTS)
        self.assertIn("u32::from_le_bytes", RUST_ARCHITECTURE_CONSTRAINTS)

    def test_constraints_block_is_the_very_first_content_in_the_prompt(self):
        # Position zero: byte-stable constraints lead every fixer prompt --
        # maximal prompt-cache prefix, and the guardrails can never be
        # truncated away (truncate_to_token_budget keeps the head).
        prompt = build_prompt(make_gap(gap_count=2))
        self.assertTrue(prompt.startswith("CRITICAL RUST ARCHITECTURE CONSTRAINTS"))

    def test_constraints_are_numbered_with_caps_labels(self):
        for label in ("STATE:", "TYPES:", "BYTES:", "TREES:", "BLOAT:", "ERRORS:", "PERL MAP:"):
            self.assertIn(label, RUST_ARCHITECTURE_CONSTRAINTS)

    def test_build_prompt_includes_the_constraints_block(self):
        prompt = build_prompt(make_gap(gap_count=2))
        self.assertIn("CRITICAL RUST ARCHITECTURE CONSTRAINTS", prompt)


class NeighborPrecedentTests(unittest.TestCase):
    def _gap(self, tmp):
        (tmp / "j.rs").write_text('metadata.insert("APP12:ColorMode".to_string(), v);')
        return {"format": "JPEG",
                "missing_tags": [{"family": "APP12", "name": "MODE3", "value": "0",
                                  "tag_id": None, "source_file": None}],
                "value_differences": [], "gap_count": 1, "parser_files": ["j.rs"]}

    def test_finds_an_implemented_sibling_literal(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            gap = self._gap(tmp)
            self.assertEqual(find_implemented_sibling(gap, tmp), "APP12:ColorMode")

    def test_own_gap_tags_are_not_their_own_precedent(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            gap = self._gap(tmp)
            # Overwrite AFTER _gap (which itself writes a ColorMode
            # literal) so the file holds only the gap's own tag.
            (tmp / "j.rs").write_text('metadata.insert("APP12:MODE3".to_string(), v);')
            self.assertIsNone(find_implemented_sibling(gap, tmp))

    def test_block_includes_the_historic_patch(self):
        calls = []
        def fake_git(args, cwd):
            calls.append(args)
            if args[0] == "log":
                return "abc123\n"
            return "commit abc123\n+++ test added here\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            block = build_neighbor_precedent_block(self._gap(tmp), tmp, git_runner_fn=fake_git)
        self.assertIn("APP12:ColorMode", block)
        self.assertIn("test added here", block)
        self.assertIn("-S", str(calls[0]))

    def test_git_failure_yields_empty_block(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            block = build_neighbor_precedent_block(
                self._gap(tmp), tmp, git_runner_fn=lambda a, c: "")
        self.assertEqual(block, "")


class BuildPromptNeighborPrecedentTests(unittest.TestCase):
    def test_block_appears_in_the_stable_section(self):
        gap = make_gap(gap_count=1)
        prompt = build_prompt(gap, neighbor_precedent_block="\n\nPRECEDENT-MARKER-XYZ")
        self.assertIn("PRECEDENT-MARKER-XYZ", prompt)
        self.assertLess(prompt.index("PRECEDENT-MARKER-XYZ"),
                        prompt.index("Previous attempts") if "Previous attempts" in prompt
                        else len(prompt))


class ParseRequestRangeTests(unittest.TestCase):
    def test_plain_path_has_no_range(self):
        self.assertEqual(parse_request_range("src/parsers/x.rs"), ("src/parsers/x.rs", None, None))

    def test_valid_range_is_parsed(self):
        self.assertEqual(parse_request_range("src/parsers/x.rs:40-120"), ("src/parsers/x.rs", 40, 120))

    def test_whitespace_is_stripped(self):
        self.assertEqual(parse_request_range("  src/x.rs:1-5  "), ("src/x.rs", 1, 5))

    def test_inverted_range_strips_suffix_and_falls_back_to_whole_file(self):
        self.assertEqual(parse_request_range("src/x.rs:9-3"), ("src/x.rs", None, None))

    def test_zero_start_strips_suffix_and_falls_back(self):
        self.assertEqual(parse_request_range("src/x.rs:0-5"), ("src/x.rs", None, None))

    def test_non_numeric_suffix_is_just_part_of_the_path(self):
        self.assertEqual(parse_request_range("src/x.rs:a-b"), ("src/x.rs:a-b", None, None))


class ResolveRequestRangeTests(unittest.TestCase):
    def _make_repo(self, tmpdir):
        repo = Path(tmpdir)
        (repo / "src").mkdir()
        (repo / "src" / "big.rs").write_text("\n".join(f"line{i}" for i in range(1, 101)))
        return repo

    def test_range_returns_numbered_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:5-7", repo, None)
        self.assertIn("Lines 5-7 of src/big.rs", answer)
        self.assertIn("5: line5", answer)
        self.assertIn("7: line7", answer)
        self.assertNotIn("line8", answer)

    def test_range_end_is_clamped_to_file_length(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:98-500", repo, None)
        self.assertIn("Lines 98-100 of src/big.rs", answer)
        self.assertIn("100: line100", answer)

    def test_range_start_past_eof_returns_guidance_not_content(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:500-600", repo, None)
        self.assertIn("only 100 lines", answer)

    def test_sample_files_ignore_ranges_and_hex_dump_whole_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            samples = Path(tmpdir)
            (samples / "EXE.dylib").write_bytes(b"\xfe\xed\xfa\xcf1234")
            answer = resolve_request("EXE.dylib:1-2", Path("/nonexistent"), samples)
        self.assertIn("Hex dump of EXE.dylib", answer)


class LoadRecentSweepReviewsTests(unittest.TestCase):
    def test_missing_file_returns_empty_list(self):
        self.assertEqual(
            load_recent_sweep_reviews(Path("/nonexistent/path.jsonl"), "NEF"), []
        )

    def test_filters_by_format_and_orders_newest_first(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entries = [
                {"format": "NEF", "tag": "A", "verdict": "accepted", "reason": "r1"},
                {"format": "JPEG", "tag": "B", "verdict": "accepted", "reason": "r2"},
                {"format": "NEF", "tag": "C", "verdict": "rejected", "reason": "r3"},
            ]
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(log_path, "NEF")
        self.assertEqual([e["tag"] for e in result], ["C", "A"])

    def test_caps_at_max_entries(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entries = [
                {"format": "NEF", "tag": f"T{i}", "verdict": "accepted", "reason": "r"}
                for i in range(10)
            ]
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(log_path, "NEF", max_entries=3)
        self.assertEqual(len(result), 3)
        self.assertEqual([e["tag"] for e in result], ["T9", "T8", "T7"])

    def test_skips_malformed_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            log_path.write_text(
                "not json\n"
                + json.dumps({"format": "NEF", "tag": "A", "verdict": "accepted", "reason": "r"})
                + "\n"
            )
            result = load_recent_sweep_reviews(log_path, "NEF")
        self.assertEqual(len(result), 1)


class FormatSweepReviewHistoryTests(unittest.TestCase):
    def test_empty_entries_returns_empty_string(self):
        self.assertEqual(format_sweep_review_history([]), "")

    def test_renders_verdict_tag_and_reason(self):
        rendered = format_sweep_review_history([
            {"format": "NEF", "tag": "ExifIFD:CFAPattern", "verdict": "rejected", "reason": "wrong name"},
        ])
        self.assertIn("REJECTED ExifIFD:CFAPattern: wrong name", rendered)


# A minimal but realistic fixture mirroring Exif.pm's actual structure: one
# table with GROUPS and two tags whose names collide on a shared prefix
# (CFAPattern / CFAPattern2) at genuinely different IDs -- the exact
# real-world case (0xA302 vs 0x828E) a fixer got backwards this session.
EXIFTOOL_PM_FIXTURE = """package Image::ExifTool::Exif;

%Image::ExifTool::Exif::Main = (
    GROUPS => { 0 => 'EXIF', 1 => 'ExifIFD', 2 => 'Image' },
    WRITE_PROC => \\&Image::ExifTool::Exif::WriteExif,
    NOTES => q{
        This table documents the standard EXIF tags found in the ExifIFD.
        See the JEITA/CIPA specification for the full tag list.
    },
    0x828e => {
        Name => 'CFAPattern2',
        Format => 'int8u',
        Protected => 1,
        Writable => 'int8u',
        Count => -1,
    },
    0xa302 => {
        Name => 'CFAPattern',
        Writable => 'undef',
        RawConv => 'Image::ExifTool::Exif::DecodeCFAPattern($self, $val)',
        PrintConv => 'Image::ExifTool::Exif::PrintCFAPattern($val)',
    },
);

1;
"""


class ExtractPerlTagSnippetTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.lib_dir = Path(self.tmpdir.name)
        (self.lib_dir / "Exif.pm").write_text(EXIFTOOL_PM_FIXTURE)

    def tearDown(self):
        self.tmpdir.cleanup()

    def test_returns_none_when_lib_dir_is_none(self):
        self.assertIsNone(extract_perl_tag_snippet("CFAPattern2", None))

    def test_returns_none_when_tag_not_found(self):
        self.assertIsNone(extract_perl_tag_snippet("NoSuchTag", self.lib_dir))

    def test_finds_tag_by_exact_name_and_includes_table_context(self):
        snippet = extract_perl_tag_snippet("CFAPattern2", self.lib_dir)
        self.assertIsNotNone(snippet)
        self.assertIn("Exif.pm", snippet)
        self.assertIn("Image::ExifTool::Exif::Main", snippet)
        self.assertIn("GROUPS => { 0 => 'EXIF', 1 => 'ExifIFD', 2 => 'Image' }", snippet)
        self.assertIn("0x828e", snippet)
        self.assertIn("Format => 'int8u'", snippet)
        # Must not bleed into the neighboring CFAPattern (0xa302) block.
        self.assertNotIn("DecodeCFAPattern", snippet)

    def test_similarly_named_tag_does_not_cross_match(self):
        """CFAPattern (0xa302) and CFAPattern2 (0x828e) must resolve to
        their own distinct blocks -- a substring/fuzzy match here would
        silently reproduce the exact bug this feature exists to prevent."""
        snippet = extract_perl_tag_snippet("CFAPattern", self.lib_dir)
        self.assertIsNotNone(snippet)
        self.assertIn("0xa302", snippet)
        self.assertIn("DecodeCFAPattern", snippet)
        self.assertNotIn("Count => -1", snippet)

    def test_tag_id_disambiguates_when_names_would_be_ambiguous(self):
        snippet = extract_perl_tag_snippet("CFAPattern2", self.lib_dir, tag_id="0x828E")
        self.assertIn("0x828e", snippet)
        self.assertIn("Format => 'int8u'", snippet)


EXE_PM_FIXTURE = """package Image::ExifTool::EXE;

%Image::ExifTool::EXE::MachO = (
    GROUPS => { 2 => 'Other' },
    NOTES => q{
        Information extracted from Mach-O (Mac OS X) executable files.
    },
    0 => 'CPUArchitecture',
    1 => 'CPUByteOrder',
);

%Image::ExifTool::EXE::PEF = (
    GROUPS => { 2 => 'Other' },
    NOTES => q{
        Information extracted from PEF (Classic MacOS) executable files.
    },
    2 => {
        Name => 'CPUArchitecture',
        Format => 'undef[4]',
        PrintConv => {
            pwpc => 'PowerPC',
            m68k => '68000',
        },
    },
);

1;
"""


class ExtractPerlTagSnippetBareFormAndFormatHintTests(unittest.TestCase):
    """Regression coverage for a real bug caught while building a
    MachO:EXE:CPUArchitecture example prompt: EXE.pm defines
    "CPUArchitecture" bare (no `Name =>` key) in its MachO table but
    with an explicit `Name =>` key in its PEF table, so a plain
    first-name-match search picked PEF's entry for a MachO gap."""

    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.lib_dir = Path(self.tmpdir.name)
        (self.lib_dir / "Exif.pm").write_text(EXIFTOOL_PM_FIXTURE)
        (self.lib_dir / "EXE.pm").write_text(EXE_PM_FIXTURE)

    def tearDown(self):
        self.tmpdir.cleanup()

    def test_bare_form_entry_is_found_by_default_document_order(self):
        # MachO's bare `0 => 'CPUArchitecture'` entry appears earlier in
        # the file than PEF's explicit `Name =>` entry, so even with no
        # format_hint it must win on true document order -- the bug this
        # regression-tests was the bare form being skipped entirely (never
        # matched at all), not merely losing a tiebreak.
        snippet = extract_perl_tag_snippet("CPUArchitecture", self.lib_dir)
        self.assertIsNotNone(snippet)
        self.assertIn("Image::ExifTool::EXE::MachO", snippet)
        self.assertIn("0 => 'CPUArchitecture'", snippet)

    def test_format_hint_prefers_matching_table_over_document_order(self):
        snippet = extract_perl_tag_snippet("CPUArchitecture", self.lib_dir, format_hint="MachO")
        self.assertIn("Image::ExifTool::EXE::MachO", snippet)
        self.assertIn("0 => 'CPUArchitecture'", snippet)
        self.assertNotIn("PowerPC", snippet)

    def test_format_hint_for_other_table_picks_that_one_instead(self):
        snippet = extract_perl_tag_snippet("CPUArchitecture", self.lib_dir, format_hint="PEF")
        self.assertIn("Image::ExifTool::EXE::PEF", snippet)
        self.assertIn("PowerPC", snippet)

    def test_unmatched_format_hint_falls_back_to_document_order(self):
        snippet = extract_perl_tag_snippet("CPUArchitecture", self.lib_dir, format_hint="ELF")
        self.assertIn("Image::ExifTool::EXE::MachO", snippet)


class BuildPerlReferenceBlockTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.lib_dir = Path(self.tmpdir.name)
        (self.lib_dir / "Exif.pm").write_text(EXIFTOOL_PM_FIXTURE)

    def tearDown(self):
        self.tmpdir.cleanup()

    def test_empty_when_lib_dir_is_none(self):
        gap = make_gap(gap_count=1)
        self.assertEqual(build_perl_reference_block(gap, None), "")

    def test_pulls_snippets_for_missing_and_diff_tags(self):
        gap = {
            "format": "NEF",
            "missing_tags": [
                {"family": "EXIF", "name": "CFAPattern2", "value": "x", "tag_id": None, "source_file": None},
            ],
            "value_differences": [
                {"tag_key": "NEF:ExifIFD:CFAPattern", "exiftool_value": "a", "oxidex_value": "b", "source_file": None},
            ],
            "gap_count": 2,
            "parser_files": [],
        }
        block = build_perl_reference_block(gap, self.lib_dir)
        self.assertIn("CFAPattern2", block)
        self.assertIn("0x828e", block)
        self.assertIn("0xa302", block)

    def test_caps_at_max_tags_shown(self):
        gap = {
            "format": "NEF",
            "missing_tags": [
                {"family": "EXIF", "name": "CFAPattern2", "value": "x", "tag_id": None, "source_file": None},
                {"family": "EXIF", "name": "CFAPattern", "value": "y", "tag_id": None, "source_file": None},
            ],
            "value_differences": [],
            "gap_count": 2,
            "parser_files": [],
        }
        block = build_perl_reference_block(gap, self.lib_dir, max_tags_shown=1)
        self.assertEqual(block.count("--- Exif.pm"), 1)

    def test_empty_when_no_tags_found_in_lib(self):
        gap = {
            "format": "NEF",
            "missing_tags": [
                {"family": "EXIF", "name": "TotallyUnknownTag", "value": "x", "tag_id": None, "source_file": None},
            ],
            "value_differences": [],
            "gap_count": 1,
            "parser_files": [],
        }
        self.assertEqual(build_perl_reference_block(gap, self.lib_dir), "")


class ExtractPerlTableNotesTests(unittest.TestCase):
    def test_finds_notes_block(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "Exif.pm"
            path.write_text(EXIFTOOL_PM_FIXTURE)
            notes = extract_perl_table_notes(path)
        self.assertIsNotNone(notes)
        self.assertIn("standard EXIF tags found in the ExifIFD", notes)

    def test_returns_none_when_no_notes_block(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "NoNotes.pm"
            path.write_text("package Image::ExifTool::NoNotes;\n%Main = (\n    0x1 => { Name => 'X' },\n);\n")
            self.assertIsNone(extract_perl_table_notes(path))

    def test_returns_none_for_missing_file(self):
        self.assertIsNone(extract_perl_table_notes(Path("/nonexistent/Foo.pm")))

    def test_handles_parenthesis_delimiter(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "Paren.pm"
            path.write_text(
                "%Main = (\n"
                "    NOTES => q(\n"
                "        This table uses parenthesis delimiters instead of braces.\n"
                "    ),\n"
                ");\n"
            )
            notes = extract_perl_table_notes(path)
        self.assertIn("parenthesis delimiters", notes)


class BuildFormatOverviewBlockTests(unittest.TestCase):
    def test_always_includes_architecture_primer(self):
        block = build_format_overview_block(None, "")
        self.assertIn(ARCHITECTURE_PRIMER, block)

    def test_includes_notes_for_modules_found_in_perl_reference_block(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            lib_dir = Path(tmpdir)
            (lib_dir / "Exif.pm").write_text(EXIFTOOL_PM_FIXTURE)
            perl_block = "--- Exif.pm, table Image::ExifTool::Exif::Main ---\n```perl\n...\n```"
            block = build_format_overview_block(lib_dir, perl_block)
        self.assertIn("standard EXIF tags found in the ExifIFD", block)

    def test_no_notes_section_when_lib_dir_is_none(self):
        block = build_format_overview_block(None, "--- Exif.pm ---\n```perl\n...\n```")
        self.assertNotIn("ExifTool's own documentation", block)

    def test_finds_module_header_when_preceded_by_lead_in_text(self):
        # Regression test: build_perl_reference_block's real output always
        # starts with an intro paragraph before the "--- module.pm ---"
        # header line -- a synthetic fixture that put the header at
        # position 0 of the string masked a bug where the header regex,
        # missing re.MULTILINE, only matched "^" at the start of the
        # whole string and therefore never found the header (and thus
        # never included NOTES) in real prompts at all.
        with tempfile.TemporaryDirectory() as tmpdir:
            lib_dir = Path(tmpdir)
            (lib_dir / "Exif.pm").write_text(EXIFTOOL_PM_FIXTURE)
            perl_block = (
                "\n\nExifTool's own Perl source for these tags (ground truth for how "
                "ExifTool actually parses/formats them):\n\n"
                "--- Exif.pm, table Image::ExifTool::Exif::Main ---\n```perl\n...\n```"
            )
            block = build_format_overview_block(lib_dir, perl_block)
        self.assertIn("standard EXIF tags found in the ExifIFD", block)


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


class FormatMemoryTests(unittest.TestCase):
    def test_load_missing_returns_empty_string(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(load_format_memory(Path(tmpdir), "NEF"), "")

    def test_append_then_load_round_trips(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            append_format_memory_note(memory_dir, "NEF", "Tried BlackLevelBlue, rejected.", now_fn=lambda: 1_700_000_000)
            text = load_format_memory(memory_dir, "NEF")
        self.assertIn("Tried BlackLevelBlue, rejected.", text)

    def test_append_accumulates_across_calls(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            append_format_memory_note(memory_dir, "NEF", "First note.")
            append_format_memory_note(memory_dir, "NEF", "Second note.")
            text = load_format_memory(memory_dir, "NEF")
        self.assertIn("First note.", text)
        self.assertIn("Second note.", text)

    def test_notes_for_different_formats_dont_mix(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            append_format_memory_note(memory_dir, "NEF", "NEF-specific note.")
            append_format_memory_note(memory_dir, "JPEG", "JPEG-specific note.")
            nef_text = load_format_memory(memory_dir, "NEF")
        self.assertIn("NEF-specific note.", nef_text)
        self.assertNotIn("JPEG-specific note.", nef_text)

    def test_summarize_no_op_when_under_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            append_format_memory_note(memory_dir, "NEF", "short")
            called = []
            did_summarize = summarize_format_memory(
                memory_dir, "NEF", CONFIG,
                call_model_fn=lambda *a, **k: called.append(1),
                max_chars=1000,
            )
        self.assertFalse(did_summarize)
        self.assertEqual(called, [])

    def test_summarize_condenses_when_over_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            for i in range(20):
                append_format_memory_note(memory_dir, "NEF", f"note number {i} with some padding text here")
            before = load_format_memory(memory_dir, "NEF")
            did_summarize = summarize_format_memory(
                memory_dir, "NEF", CONFIG,
                call_model_fn=lambda messages, *a, **k: "- Condensed: use IFD0: prefix for RAW tags.",
                max_chars=200,
            )
            after = load_format_memory(memory_dir, "NEF")
        self.assertTrue(did_summarize)
        self.assertLess(len(after), len(before))
        self.assertIn("Condensed: use IFD0: prefix", after)

    def test_summarize_leaves_memory_untouched_on_call_failure(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            for i in range(20):
                append_format_memory_note(memory_dir, "NEF", f"note number {i} with some padding text here")
            before = load_format_memory(memory_dir, "NEF")

            def raising(*a, **k):
                raise TimeoutError("timed out")

            did_summarize = summarize_format_memory(
                memory_dir, "NEF", CONFIG, call_model_fn=raising, max_chars=200,
            )
            after = load_format_memory(memory_dir, "NEF")
        self.assertFalse(did_summarize)
        self.assertEqual(before, after)

    def test_summarize_leaves_memory_untouched_on_empty_reply(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            for i in range(20):
                append_format_memory_note(memory_dir, "NEF", f"note number {i} with some padding text here")
            before = load_format_memory(memory_dir, "NEF")
            did_summarize = summarize_format_memory(
                memory_dir, "NEF", CONFIG, call_model_fn=lambda *a, **k: "   ", max_chars=200,
            )
            after = load_format_memory(memory_dir, "NEF")
        self.assertFalse(did_summarize)
        self.assertEqual(before, after)


class BuildPromptFormatMemoryTests(unittest.TestCase):
    def test_omitted_when_dir_not_given(self):
        prompt = build_prompt(make_gap(gap_count=1))
        self.assertNotIn("Accumulated notes from previous rounds", prompt)

    def test_included_when_present(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            memory_dir = Path(tmpdir)
            append_format_memory_note(memory_dir, "NEF", "Watch out for hardcoded prefixes.")
            prompt = build_prompt(make_gap(gap_count=1), format_memory_dir=memory_dir)
        self.assertIn("Accumulated notes from previous rounds", prompt)
        self.assertIn("Watch out for hardcoded prefixes.", prompt)

    def test_omitted_when_dir_given_but_empty(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            prompt = build_prompt(make_gap(gap_count=1), format_memory_dir=Path(tmpdir))
        self.assertNotIn("Accumulated notes from previous rounds", prompt)


class FormatPreviousAttemptsTests(unittest.TestCase):
    def test_empty_returns_empty_string(self):
        self.assertEqual(format_previous_attempts([]), "")
        self.assertEqual(format_previous_attempts(None), "")

    def test_includes_reason_without_critique(self):
        rendered = format_previous_attempts([{"diff": "d", "reason": "build failed"}])
        self.assertIn("Failed because: build failed", rendered)
        self.assertNotIn("Reviewer critique", rendered)

    def test_includes_critique_when_present(self):
        rendered = format_previous_attempts([
            {"diff": "d", "reason": "test_regressed", "critique": "off-by-one in the loop bound"},
        ])
        self.assertIn("Failed because: test_regressed", rendered)
        self.assertIn("Reviewer critique: off-by-one in the loop bound", rendered)


class BuildFailureCritiquePromptTests(unittest.TestCase):
    def test_includes_failure_kind_detail_and_diff(self):
        gap = make_gap()
        prompt = build_failure_critique_prompt(gap, "--- a/x\n+++ b/x\n", "build_failed", "compile error: E0308")
        self.assertIn("build_failed", prompt)
        self.assertIn("compile error: E0308", prompt)
        self.assertIn("--- a/x\n+++ b/x\n", prompt)

    def test_handles_missing_diff(self):
        gap = make_gap()
        prompt = build_failure_critique_prompt(gap, None, "build_failed", "no diff in model response")
        self.assertIn("No diff was produced", prompt)


class CritiqueFailedAttemptTests(unittest.TestCase):
    def test_returns_model_critique(self):
        gap = make_gap()
        critique = critique_failed_attempt(
            gap, "--- a/x\n+++ b/x\n", "test_regressed", "assertion failed",
            {"base_url": "u", "api_key": "k", "models": [{"name": "m", "base_url": "u", "api_key": "k"}],
             "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=lambda messages, *a: "  Root cause: off-by-one in the loop bound.  ",
        )
        self.assertEqual(critique, "Root cause: off-by-one in the loop bound.")

    def test_falls_back_to_failure_detail_when_call_fails(self):
        gap = make_gap()

        def raising(messages, *a):
            raise TimeoutError("timed out")

        critique = critique_failed_attempt(
            gap, "--- a/x\n+++ b/x\n", "build_failed", "compile error: E0308",
            {"base_url": "u", "api_key": "k", "models": [{"name": "m", "base_url": "u", "api_key": "k"}],
             "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=raising,
        )
        self.assertEqual(critique, "compile error: E0308")


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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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

    def test_reassembles_a_diff_sent_as_patch_chunks(self):
        replies = [
            "PATCH 1/2\n```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n```\n",
            "PATCH 2/2\n```diff\n+new\n```\n",
        ]
        calls = []

        def fake_call_model(messages, *a):
            calls.append(1)
            return replies[len(calls) - 1]

        applied_diffs = []

        def fake_git_apply(diff, root):
            applied_diffs.append(diff)
            return True, "ok"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=fake_git_apply,
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(calls), 2)
        # The two chunks concatenate back into exactly the original diff.
        self.assertEqual(applied_diffs, ["--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n"])
        self.assertEqual(diff, applied_diffs[0])

    def test_prompts_for_the_next_chunk_between_patch_messages(self):
        def fake_call_model(messages, *a):
            if len(messages) == 1:
                return "PATCH 1/2\n```diff\n--- a/x\n+++ b/x\n```\n"
            # Second call: the harness must have asked for the missing chunk.
            self.assertIn("missing chunk(s) 2", messages[-1]["content"])
            return "PATCH 2/2\n```diff\n@@ -1 +1 @@\n-old\n+new\n```\n"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)

    def test_asks_to_resend_a_patch_chunk_missing_its_diff_fence(self):
        replies = [
            "PATCH 1/2\nI'll send the diff shortly.",  # malformed -- no ```diff block
            "PATCH 1/2\n```diff\n--- a/x\n+++ b/x\n```\n",
            "PATCH 2/2\n```diff\n@@ -1 +1 @@\n-old\n+new\n```\n",
        ]
        calls = []

        def fake_call_model(messages, *a):
            calls.append(1)
            if len(calls) == 2:
                self.assertIn("didn't include a", messages[-1]["content"])
            return replies[len(calls) - 1]

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(calls), 3)

    def test_handles_out_of_order_chunk_delivery(self):
        # Chunk 3/3 arrives before chunk 2/3 -- reassembly must notice the
        # gap (not just check "is this reply's own index the last one")
        # and, once complete, concatenate in INDEX order, not receipt order.
        replies = [
            "PATCH 1/3\n```diff\n--- a/x\n```\n",
            "PATCH 3/3\n```diff\n+new\n```\n",
            "PATCH 2/3\n```diff\n+++ b/x\n@@ -1 +1 @@\n-old\n```\n",
        ]
        calls = []

        def fake_call_model(messages, *a):
            calls.append(1)
            if len(calls) == 2:
                self.assertIn("missing chunk(s) 2, 3", messages[-1]["content"])
            if len(calls) == 3:
                self.assertIn("missing chunk(s) 2", messages[-1]["content"])
            return replies[len(calls) - 1]

        applied_diffs = []

        def fake_git_apply(diff, root):
            applied_diffs.append(diff)
            return True, "ok"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=fake_git_apply,
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(calls), 3)
        self.assertEqual(applied_diffs, ["--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n"])

    def test_fails_with_specific_reason_when_patch_chunking_exceeds_safety_limit(self):
        # A misbehaving/looping model keeps declaring more chunks than any
        # reasonable diff would need -- this must not stall the attempt
        # forever.
        def fake_call_model(messages, *a):
            return "PATCH 1/1000\n```diff\n--- a/x\n+++ b/x\n```\n"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertIn("patch chunking exceeded safety limit", reason)

    def test_rejects_a_patch_header_with_index_greater_than_total(self):
        def fake_call_model(messages, *a):
            return "PATCH 5/2\n```diff\n--- a/x\n+++ b/x\n```\n"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertIn("patch chunking exceeded safety limit", reason)

    def test_third_identical_request_gets_pivot_nudge_instead_of_content(self):
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) <= 3:
                return "REQUEST: src/parsers/jpeg/mod.rs"
            # 4th call: after the pivot nudge, submit a diff.
            self.assertIn("Pivot:", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src" / "parsers" / "jpeg").mkdir(parents=True)
            (repo / "src" / "parsers" / "jpeg" / "mod.rs").write_text("real content")
            built, reason, diff, messages = attempt_build(
                [{"role": "user", "content": "fix format X"}],
                call_model_fn=fake_call_model,
                git_apply_fn=lambda diff, root: (True, "ok"),
                git_checkout_clean_fn=lambda root: None,
                cargo_build_fn=lambda root: (True, ""),
                config=CONFIG,
                repo_root=repo,
            )
        self.assertTrue(built)
        # Turns 1 and 2 served content; turn 3 got the nudge, not content.
        served_turns = [m for m in messages if m["role"] == "user" and "real content" in m["content"]]
        nudge_turns = [m for m in messages if m["role"] == "user" and "Pivot:" in m["content"]]
        self.assertEqual(len(served_turns), 2)
        self.assertEqual(len(nudge_turns), 1)

    def test_distinct_requests_do_not_trigger_the_pivot_nudge(self):
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 1:
                return "REQUEST: src/a.rs"
            if len(replies) == 2:
                return "REQUEST: src/b.rs"
            self.assertNotIn("Pivot:", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)

    def test_max_request_repeats_is_configurable(self):
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 2:
                self.assertIn("Pivot:", messages[-1]["content"])
                return "```diff\n--- a/x\n+++ b/x\n```\n"
            return "REQUEST: src/x.rs"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, max_request_repeats=1),
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)

    def test_conversation_is_compacted_when_over_the_trigger(self):
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) <= 2:
                return f"REQUEST: src/a{len(replies)}.rs"
            # By the 3rd call, the first served payload must be stubbed.
            stub_turns = [m for m in messages if "[earlier content elided for space:" in m["content"]]
            self.assertGreaterEqual(len(stub_turns), 1)
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "a1.rs").write_text("y" * 60_000)
            (repo / "src" / "a2.rs").write_text("y" * 60_000)
            built, reason, diff, messages = attempt_build(
                [{"role": "user", "content": "fix format X"}],
                call_model_fn=fake_call_model,
                git_apply_fn=lambda diff, root: (True, "ok"),
                git_checkout_clean_fn=lambda root: None,
                cargo_build_fn=lambda root: (True, ""),
                config=dict(CONFIG, compaction_trigger_tokens=5000, compaction_keep_recent_turns=1),
                repo_root=repo,
            )
        self.assertTrue(built)


class CargoCheckTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_returns_success_and_combined_output(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="checked ok\n", stderr="warn\n")
        ok, output = cargo_check(Path("/fake/repo"))
        self.assertTrue(ok)
        self.assertEqual(output, "checked ok\nwarn\n")
        self.assertEqual(mock_run.call_args[0][0], ["cargo", "check", "--workspace"])

    @patch("model_fix_loop.subprocess.run")
    def test_nonzero_exit_is_failure(self, mock_run):
        mock_run.return_value = MagicMock(returncode=101, stdout="", stderr="error[E0308]\n")
        ok, output = cargo_check(Path("/fake/repo"))
        self.assertFalse(ok)
        self.assertIn("E0308", output)


class AttemptBuildVerifyTests(unittest.TestCase):
    def _run(self, fake_call_model, cargo_check_fn, config=None, git_apply_fn=None):
        cleans = []
        return attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=git_apply_fn or (lambda diff, root: (True, "ok")),
            git_checkout_clean_fn=lambda root: cleans.append(1),
            cargo_build_fn=lambda root: (True, ""),
            config=config or CONFIG,
            repo_root=Path("/fake/repo"),
            cargo_check_fn=cargo_check_fn,
        ), cleans

    def test_verify_applies_checks_reverts_and_reports(self):
        checks = []
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 1:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n```\n"
            self.assertIn("cargo check FAILED", messages[-1]["content"])
            self.assertIn("mismatched types", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        def fake_check(root):
            checks.append(1)
            return False, "error[E0308]: mismatched types"

        (built, reason, diff, messages), cleans = self._run(fake_call_model, fake_check)
        self.assertTrue(built)
        self.assertEqual(len(checks), 1)
        self.assertGreaterEqual(len(cleans), 1)  # trial change was reverted

    def test_verify_passing_check_reports_passed(self):
        def fake_call_model(messages, *a):
            if len(messages) == 1:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n"
            self.assertIn("cargo check PASSED", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, _, _, _), _ = self._run(fake_call_model, lambda root: (True, "clean"))
        self.assertTrue(built)

    def test_verify_never_consumes_a_diff_attempt(self):
        # Two VERIFYs then two failing real diffs: the 2-diff-attempt
        # budget must still allow both real diffs.
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) <= 2:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n"
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, reason, _, _), _ = self._run(
            lambda m, *a: fake_call_model(m, *a),
            lambda root: (True, "ok"),
        )
        # 3rd call is a real diff that applies and builds -> success.
        self.assertTrue(built)
        self.assertEqual(len(replies), 3)

    def test_verify_without_cargo_check_fn_gets_unavailable_message(self):
        def fake_call_model(messages, *a):
            if len(messages) == 1:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n"
            self.assertIn("VERIFY is unavailable", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, _, _, _), _ = self._run(fake_call_model, None)
        self.assertTrue(built)

    def test_verify_with_no_diff_block_consumes_a_turn_and_asks_again(self):
        def fake_call_model(messages, *a):
            if len(messages) == 1:
                return "VERIFY\nI'll test changing the offset."
            self.assertIn("no ```diff fenced block", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, _, _, _), _ = self._run(fake_call_model, lambda root: (True, "ok"))
        self.assertTrue(built)

    def test_verify_budget_exhaustion_demands_final_diff_then_fails_on_refusal(self):
        result, _ = self._run(
            lambda messages, *a: "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n",
            lambda root: (True, "ok"),
            config=dict(CONFIG, max_verify_turns=2),
        )
        built, reason, diff, messages = result
        self.assertFalse(built)
        self.assertIn("verify budget", reason)


class ModelsForPhaseTests(unittest.TestCase):
    TERRA = {"name": "gpt-5.6-terra", "base_url": "u", "api_key": "k", "phase": "explore", "reasoning_effort": "medium"}
    SOL = {"name": "gpt-5.6-sol", "base_url": "u", "api_key": "k", "phase": "patch", "reasoning_effort": "max"}
    UNTAGGED = {"name": "any", "base_url": "u", "api_key": "k"}

    def test_filters_to_matching_phase(self):
        pool = [self.TERRA, self.SOL]
        self.assertEqual(models_for_phase(pool, "explore"), [self.TERRA])
        self.assertEqual(models_for_phase(pool, "patch"), [self.SOL])

    def test_untagged_entries_belong_to_every_phase(self):
        pool = [self.TERRA, self.UNTAGGED]
        self.assertEqual(models_for_phase(pool, "patch"), [self.UNTAGGED])
        self.assertEqual(models_for_phase(pool, "explore"), [self.TERRA, self.UNTAGGED])

    def test_empty_filter_falls_back_to_full_pool(self):
        pool = [self.TERRA]
        self.assertEqual(models_for_phase(pool, "patch"), [self.TERRA])


class ModelSpecPhaseTests(unittest.TestCase):
    def test_phase_and_reasoning_effort_are_accepted(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k",
            "models": [{"name": "m", "phase": "explore", "reasoning_effort": "medium"}],
        })
        self.assertEqual(config["models"][0]["phase"], "explore")
        self.assertEqual(config["models"][0]["reasoning_effort"], "medium")

    def test_missing_phase_and_effort_default_to_none(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["bare-name"],
        })
        self.assertIsNone(config["models"][0].get("phase"))
        self.assertIsNone(config["models"][0].get("reasoning_effort"))

    def test_invalid_phase_raises_at_load(self):
        with self.assertRaises(ValueError):
            _normalize_model_config({
                "base_url": "u", "api_key": "k",
                "models": [{"name": "m", "phase": "turbo"}],
            })

    def test_unknown_key_still_raises(self):
        with self.assertRaises(ValueError):
            _normalize_model_config({
                "base_url": "u", "api_key": "k",
                "models": [{"name": "m", "max_tokens": 4096}],
            })


class AttemptBuildPhaseRoutingTests(unittest.TestCase):
    TERRA = {"name": "gpt-5.6-terra", "base_url": "u", "api_key": "k", "phase": "explore", "reasoning_effort": "medium"}
    SOL = {"name": "gpt-5.6-sol", "base_url": "u", "api_key": "k", "phase": "patch", "reasoning_effort": "max"}

    def test_explore_then_patch_pools_across_a_repair(self):
        pools_seen = []

        def tracking_pick(models):
            pools_seen.append([m["name"] for m in models])
            return models[0]

        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 1:
                return "REQUEST: src/a.rs"
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        apply_results = iter([(False, "does not apply"), (True, "ok")])
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: next(apply_results),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, models=[self.TERRA, self.SOL]),
            repo_root=Path("/fake/repo"),
            pick_model_fn=tracking_pick,
        )
        self.assertTrue(built)
        # Call 1: fresh attempt -> explore (terra). Call 2: after a served
        # REQUEST answer -> still explore. Call 3: after an apply-failure
        # repair prompt -> patch (sol).
        self.assertEqual(pools_seen[0], ["gpt-5.6-terra"])
        self.assertEqual(pools_seen[1], ["gpt-5.6-terra"])
        self.assertEqual(pools_seen[2], ["gpt-5.6-sol"])

    def test_reinvocation_with_existing_conversation_starts_in_patch_phase(self):
        pools_seen = []

        def tracking_pick(models):
            pools_seen.append([m["name"] for m in models])
            return models[0]

        built, reason, diff, messages = attempt_build(
            [
                {"role": "user", "content": "fix format X"},
                {"role": "assistant", "content": "```diff\nbad\n```"},
                {"role": "user", "content": "That attempt failed (build_failed): ...\nPlease resend a corrected diff."},
            ],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, models=[self.TERRA, self.SOL]),
            repo_root=Path("/fake/repo"),
            pick_model_fn=tracking_pick,
        )
        self.assertTrue(built)
        self.assertEqual(pools_seen[0], ["gpt-5.6-sol"])

    def test_per_entry_reasoning_effort_reaches_the_call(self):
        efforts_seen = []

        def fake_call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort, *a):
            efforts_seen.append(reasoning_effort)
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        built, *_ = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, models=[self.TERRA]),
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(efforts_seen, ["medium"])


class CritiqueUsesExploreTierTests(unittest.TestCase):
    def test_critique_picks_from_the_explore_pool(self):
        terra = {"name": "terra", "base_url": "u", "api_key": "k", "phase": "explore"}
        sol = {"name": "sol", "base_url": "u", "api_key": "k", "phase": "patch"}
        pools_seen = []

        def tracking_pick(models):
            pools_seen.append([m["name"] for m in models])
            return models[0]

        config = dict(CONFIG, models=[terra, sol])
        critique = critique_failed_attempt(
            make_gap(gap_count=1), "--- a/x\n", "build_failed", "error", config,
            call_model_fn=lambda *a: "try a different offset",
            pick_model_fn=tracking_pick,
        )
        self.assertEqual(critique, "try a different offset")
        self.assertEqual(pools_seen, [["terra"]])


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
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (False, "test output"),
            review_fn=lambda g, diff, config, **kwargs: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("cargo test --workspace regressed", result["reason"])
        self.assertIn("test output", result["reason"])


class FixGapTestOrderingTests(unittest.TestCase):
    def test_targeted_runs_before_review_full_suite_only_before_commit(self):
        order = []
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 0,
            cargo_test_targeted_fn=lambda root, f: (order.append("targeted"), (True, ""))[1],
            cargo_test_workspace_fn=lambda root: (order.append("full"), (True, ""))[1],
            review_fn=lambda *a, **k: (order.append("review"), (True, ""))[1],
            git_commit_fn=lambda msg, root: order.append("commit"),
            git_checkout_clean_fn=lambda root: None,
            detect_duplicate_fn=lambda *a: False,
            log_fn=lambda s: None,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(order, ["targeted", "review", "full", "commit"])

    def test_targeted_failure_is_a_test_regressed_round_without_full_suite(self):
        full_runs = []
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 0,
            cargo_test_targeted_fn=lambda root, f: (False, "targeted boom"),
            cargo_test_workspace_fn=lambda root: (full_runs.append(1), (True, ""))[1],
            git_checkout_clean_fn=lambda root: None,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("targeted boom", result["reason"])
        self.assertEqual(full_runs, [])

    def test_full_suite_failure_before_commit_reverts_and_fails_the_round(self):
        commits = []
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 0,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (False, "full boom"),
            review_fn=lambda *a, **k: (True, ""),
            git_commit_fn=lambda msg, root: commits.append(1),
            git_checkout_clean_fn=lambda root: None,
            detect_duplicate_fn=lambda *a: False,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("full boom", result["reason"])
        self.assertEqual(commits, [])


class FixGapCritiqueTests(unittest.TestCase):
    """Every non-fixed round -- not just a review rejection -- now gets a
    critique and a chance to retry, up to max_repair_rounds. See fix_gap's
    critique_and_continue helper."""

    def test_test_regression_critique_sees_actual_failure_output(self):
        gap = make_gap(gap_count=2)
        critique_reasons = []

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            critique_fn=lambda g, diff, fk, reason, cfg, **kwargs: critique_reasons.append(reason) or "critique",
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (False, "thread 'x' panicked: assertion failed"),
            review_fn=lambda g, diff, config, **kwargs: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("thread 'x' panicked: assertion failed", critique_reasons[0])

    def test_build_failure_gets_critiqued_and_retried(self):
        gap = make_gap(gap_count=2)
        critique_calls = []

        def fake_critique(g, diff, failure_kind, reason, cfg, **kwargs):
            critique_calls.append((failure_kind, reason))
            return f"critique for {failure_kind}"

        attempt_count = [0]

        def fake_attempt_build(messages, **kwargs):
            attempt_count[0] += 1
            if attempt_count[0] < 3:
                return False, "compile error: mismatched types", None, messages
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            critique_fn=fake_critique,
            review_fn=lambda g, diff, config, **kwargs: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(critique_calls), 2)  # rounds 1 and 2 failed to build
        self.assertEqual(critique_calls[0][0], "build_failed")
        self.assertEqual(len(result["rounds"]), 2)
        self.assertEqual(result["rounds"][0]["reason"], "compile error: mismatched types")
        self.assertEqual(result["rounds"][0]["critique"], "critique for build_failed")

    def test_critique_is_fed_back_into_the_conversation(self):
        gap = make_gap(gap_count=2)
        seen_messages = []

        def fake_attempt_build(messages, **kwargs):
            seen_messages.append(list(messages))
            if len(seen_messages) == 1:
                return False, "gap_not_closed reason", None, messages
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            critique_fn=lambda g, diff, fk, reason, cfg, **kwargs: "root cause: X, try Y instead",
            review_fn=lambda g, diff, config, **kwargs: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(len(seen_messages), 2)
        second_call_messages = seen_messages[1]
        last_user_msg = [m for m in second_call_messages if m["role"] == "user"][-1]
        self.assertIn("root cause: X, try Y instead", last_user_msg["content"])

    def test_gives_up_after_max_repair_rounds_with_full_round_history(self):
        gap = make_gap(gap_count=2)
        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (False, "always broken", None, messages),
            critique_fn=lambda g, diff, fk, reason, cfg, **kwargs: f"critique: {reason}",
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            repo_root=Path("/fake/repo"),
            max_repair_rounds=3,
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(len(result["rounds"]), 3)
        self.assertTrue(all(r["critique"] == "critique: always broken" for r in result["rounds"]))

    def test_duplicate_short_circuits_without_consuming_a_critique(self):
        gap = make_single_tag_gap_dict()
        critique_calls = []
        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            critique_fn=lambda *a, **kwargs: critique_calls.append(1),
            detect_duplicate_fn=lambda diff, tag, root: True,
            git_checkout_clean_fn=lambda root: None,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "duplicate")
        self.assertEqual(critique_calls, [])

    def test_infra_failure_skips_the_critique_model_call(self):
        # Critiquing a rate-limit error wastes a model call that will
        # usually itself be rate-limited, and produces no signal about
        # the tag or the diff -- fix_gap must use the reason itself as
        # the critique instead of calling critique_fn.
        gap = make_gap(gap_count=2)
        critique_calls = []
        infra_reason = "model call failed: HTTP Error 429: Too Many Requests"

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (False, infra_reason, None, messages),
            critique_fn=lambda *a, **kwargs: critique_calls.append(1) or "should never be used",
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            repo_root=Path("/fake/repo"),
            max_repair_rounds=2,
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(critique_calls, [])
        self.assertEqual(len(result["rounds"]), 2)
        for r in result["rounds"]:
            self.assertEqual(r["critique"], infra_reason)


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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
            max_repair_rounds=2,
        )

        self.assertEqual(result["status"], "failed")
        self.assertIn("rejected by review", result["reason"])
        self.assertIn("hardcodes the sample value", result["reason"])
        self.assertEqual(len(result["rounds"]), 2)

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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
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


class ClusterKeyTests(unittest.TestCase):
    def test_family_is_the_middle_component(self):
        tg = {"format": "RW2", "tag_key": "RW2:EXIF:BlackLevelRed", "parser_files": ["a.rs"]}
        self.assertEqual(cluster_key(tg), ("RW2", "EXIF", ("a.rs",)))

    def test_different_parser_files_do_not_cluster(self):
        a = {"format": "F", "tag_key": "F:X:A", "parser_files": ["a.rs"]}
        b = {"format": "F", "tag_key": "F:X:B", "parser_files": ["b.rs"]}
        self.assertNotEqual(cluster_key(a), cluster_key(b))


class MakeClusterGapTests(unittest.TestCase):
    def _tg(self, name, kind="missing"):
        if kind == "missing":
            entry = {"family": "APP12", "name": name, "value": "1", "tag_id": None, "source_file": None}
        else:
            entry = {"tag_key": f"APP12:{name}", "exiftool_value": "1", "oxidex_value": "0", "source_file": None}
        return {"format": "JPEG", "tag_key": f"JPEG:APP12:{name}", "kind": kind,
                "entry": entry, "parser_files": ["j.rs"]}

    def test_leader_without_members_matches_single_tag_gap(self):
        leader = self._tg("MODE3")
        gap = make_cluster_gap(leader)
        self.assertEqual(gap["gap_count"], 1)
        self.assertTrue(gap["clustered"])
        self.assertEqual(len(gap["missing_tags"]), 1)

    def test_members_are_unioned_across_kinds(self):
        leader = self._tg("MODE3")
        leader["cluster_members"] = [self._tg("MODE4"), self._tg("MODE5", kind="diff")]
        gap = make_cluster_gap(leader)
        self.assertEqual(gap["gap_count"], 3)
        self.assertEqual(len(gap["missing_tags"]), 2)
        self.assertEqual(len(gap["value_differences"]), 1)


class TagStillOpenTests(unittest.TestCase):
    MISSING_GAP = {"format": "XMP", "kind": "missing", "tag_key": "XMP:XMP:ArtworkTitle",
                   "entry": {"family": "XMP", "name": "ArtworkTitle", "value": "test",
                             "tag_id": None, "source_file": None},
                   "parser_files": []}
    DIFF_GAP = {"format": "RW2", "kind": "diff", "tag_key": "RW2:EXIF:ISO",
                "entry": {"tag_key": "EXIF:ISO", "exiftool_value": "100",
                          "oxidex_value": "0", "source_file": None},
                "parser_files": []}

    def test_no_match_for_format_means_closed(self):
        self.assertIsNone(tag_still_open(None, self.MISSING_GAP))

    def test_still_missing(self):
        match = {"missing_tags": [{"family": "XMP", "name": "ArtworkTitle"}],
                 "value_differences": []}
        self.assertEqual(tag_still_open(match, self.MISSING_GAP), ("missing",))

    def test_missing_tag_that_arrived_with_wrong_value_is_STILL_OPEN(self):
        # The ArtworkTitle escape: leaves missing_in_oxidex, lands in
        # value_differences with the wrong value -- must NOT count closed.
        match = {"missing_tags": [],
                 "value_differences": [{"tag_key": "XMP:ArtworkTitle",
                                        "exiftool_value": "test",
                                        "oxidex_value": "test, verfänglich"}]}
        self.assertEqual(
            tag_still_open(match, self.MISSING_GAP),
            ("value_differs", "test", "test, verfänglich"),
        )

    def test_diff_tag_still_differing(self):
        match = {"missing_tags": [],
                 "value_differences": [{"tag_key": "EXIF:ISO",
                                        "exiftool_value": "100", "oxidex_value": "0"}]}
        self.assertEqual(tag_still_open(match, self.DIFF_GAP),
                         ("value_differs", "100", "0"))

    def test_fully_closed(self):
        match = {"missing_tags": [], "value_differences": []}
        self.assertIsNone(tag_still_open(match, self.MISSING_GAP))
        self.assertIsNone(tag_still_open(match, self.DIFF_GAP))


class FixGapRecheckDetailTests(unittest.TestCase):
    def test_tuple_recheck_detail_becomes_the_failure_reason(self):
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: (1, 'target still wrong: expected "test", got "test, x"'),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn('expected "test"', result["reason"])

    def test_plain_int_recheck_still_works(self):
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 1,
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("gap count did not decrease", result["reason"])


class LoadLandedTagsTests(unittest.TestCase):
    def test_missing_file_is_empty_set(self):
        self.assertEqual(load_landed_tags(Path("/nonexistent/landed.log")), set())

    def test_parses_tag_keys_skipping_malformed_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            p = Path(tmpdir) / "landed.log"
            p.write_text("2026-07-23T17:00:00 JPEG:APP12:MODE3\n\ngarbage-no-space\n"
                         "2026-07-23T17:05:00 PSD:EXIF:Compression\n")
            self.assertEqual(load_landed_tags(p),
                             {"JPEG:APP12:MODE3", "PSD:EXIF:Compression"})


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

    def test_persists_every_internal_round_with_its_own_critique(self):
        """fix_gap's own "rounds" (its internal repair sub-attempts, each
        with a critique -- see FixGapCritiqueTests) all get persisted as
        separate attempts, not flattened into one entry per run_tag_loop
        call."""
        gaps = [make_gap()]

        def fake_fix(tag_gap, config, previous_attempts=None):
            if tag_gap["tag_key"] == "NEF:EXIF:LensModel":
                return {
                    "status": "failed", "reason": "final reason", "diff": "final-diff",
                    "rounds": [
                        {"diff": "diff-a", "reason": "build_failed: syntax error", "critique": "missing semicolon"},
                        {"diff": "diff-b", "reason": "test_regressed", "critique": "broke an unrelated test"},
                    ],
                }
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, max_fails=10,
        )
        attempts = store["NEF:EXIF:LensModel"]["attempts"]
        self.assertEqual(len(attempts), 2)
        self.assertEqual(attempts[0]["critique"], "missing semicolon")
        self.assertEqual(attempts[1]["critique"], "broke an unrelated test")

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

    def test_landed_tag_is_skipped_and_its_state_cleared(self):
        # A tag the sweep already landed (present in the landed-tags log)
        # must never be attempted again -- it's skipped like a duplicate:
        # state entry popped, and the skip logged.
        gaps = [make_gap()]  # LensModel (missing) + ISO (diff)
        attempts = []
        logged = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            attempts.append(tag_gap["tag_key"])
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        store["NEF:EXIF:LensModel"] = {"fails": 1, "blacklisted": False, "attempts": []}
        with tempfile.TemporaryDirectory() as tmpdir:
            landed = Path(tmpdir) / "landed.log"
            landed.write_text("2026-07-23T17:00:00 NEF:EXIF:LensModel\n")
            run_tag_loop(
                {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
                state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
                max_rounds=1, log_fn=logged.append, landed_tags_path=landed,
            )
        # fix_gap_fn only ever sees the OTHER tag -- the landed one is
        # filtered out before selection.
        self.assertEqual(attempts, ["NEF:EXIF:ISO"])
        # The landed tag's stale state entry (fail count, any claim) is
        # popped, same cleanup as a duplicate.
        self.assertNotIn("NEF:EXIF:LensModel", store)
        self.assertTrue(any("already landed via sweep" in line for line in logged))

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

    def _cluster_gaps(self):
        # Three same-family siblings plus one outsider, all in one
        # format-level gap so every tag shares parser_files (the third
        # cluster_key component); COM:Other differs in family, so it
        # must never ride along with the APP12 cluster.
        def entry(family, name):
            return {"family": family, "name": name, "value": "1", "tag_id": None, "source_file": None}
        return [{
            "format": "JPEG",
            "missing_tags": [entry("APP12", "MODE3"), entry("APP12", "MODE4"),
                             entry("APP12", "MODE5"), entry("COM", "Other")],
            "value_differences": [],
            "gap_count": 4,
            "parser_files": ["j.rs"],
        }]

    def test_clusters_siblings_onto_the_leader(self):
        seen = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            seen.append([m["tag_key"] for m in [tag_gap] + tag_gap.get("cluster_members", [])])
            return {"status": "fixed", "gaps_closed": 1, "rounds": []}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=self._cluster_gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, max_cluster_tags=6,
        )
        self.assertEqual(sorted(seen[0]),
                         ["JPEG:APP12:MODE3", "JPEG:APP12:MODE4", "JPEG:APP12:MODE5"])

    def test_fixed_clears_state_for_every_member(self):
        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "fixed", "gaps_closed": 1, "rounds": []}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=self._cluster_gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, max_cluster_tags=6,
        )
        for key in ("JPEG:APP12:MODE3", "JPEG:APP12:MODE4", "JPEG:APP12:MODE5"):
            self.assertNotIn(key, store)

    def test_failure_charges_only_the_leader(self):
        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=self._cluster_gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, max_cluster_tags=6,
        )
        self.assertEqual(store["JPEG:APP12:MODE3"]["fails"], 1)
        for key in ("JPEG:APP12:MODE4", "JPEG:APP12:MODE5"):
            self.assertEqual(store[key].get("fails", 0), 0)
            self.assertNotIn("claimed_by", store[key])
            self.assertNotIn("claimed_at", store[key])

    def test_max_cluster_tags_1_disables_clustering(self):
        seen = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            seen.append(tag_gap)
            return {"status": "fixed", "gaps_closed": 1, "rounds": []}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=self._cluster_gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1,
        )
        self.assertNotIn("cluster_members", seen[0])


class RunTagLoopInfraFailureTests(unittest.TestCase):
    """An infrastructure failure (attempt_build's "model call failed:"
    reason -- rate limit, network, provider error) says nothing about
    the tag or the diff, so it must not be charged against the tag's
    fail budget or persisted into its attempt history -- otherwise a
    rate-limit storm blacklists every active tag and litters each tag's
    prompt-visible history with junk 429 entries."""

    _state_io = RunTagLoopTests._state_io

    def test_pure_infra_failure_does_not_increment_fails_or_blacklist(self):
        gaps = [make_gap()]
        infra_reason = "model call failed: HTTP Error 429: Too Many Requests"

        def fake_fix(tag_gap, config, previous_attempts=None):
            return {
                "status": "failed", "reason": infra_reason, "diff": None,
                "rounds": [{"diff": None, "reason": infra_reason, "critique": None}],
            }

        store, load, save = self._state_io()
        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=4, max_fails=2,
        )
        # 4 pure-infra rounds against max_fails=2 would have blacklisted
        # twice over if they counted -- the tag must come out untouched.
        entry = store.get("NEF:EXIF:LensModel", {})
        self.assertEqual(entry.get("fails", 0), 0)
        self.assertFalse(entry.get("blacklisted", False))
        self.assertEqual(entry.get("attempts", []), [])
        # Still reported in this run's summary -- just not charged.
        self.assertEqual(len(result["failed"]), 4)

    def test_mixed_result_counts_fail_but_drops_infra_rounds_from_attempts(self):
        gaps = [make_gap()]

        def fake_fix(tag_gap, config, previous_attempts=None):
            if tag_gap["tag_key"] == "NEF:EXIF:LensModel":
                return {
                    "status": "failed", "reason": "cargo build failed: error[E0308]",
                    "diff": "diff-real",
                    "rounds": [
                        {"diff": None,
                         "reason": "model call failed: HTTP Error 429: Too Many Requests",
                         "critique": None},
                        {"diff": "diff-real", "reason": "cargo build failed: error[E0308]",
                         "critique": "type mismatch"},
                    ],
                }
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path="/fake/state.json", load_state_fn=load, save_state_fn=save,
            max_rounds=1, max_fails=10,
        )
        # The real build failure still costs a fail, but only the round
        # with real signal is persisted -- the 429 noise is dropped.
        entry = store["NEF:EXIF:LensModel"]
        self.assertEqual(entry["fails"], 1)
        self.assertEqual(len(entry["attempts"]), 1)
        self.assertEqual(entry["attempts"][0]["reason"], "cargo build failed: error[E0308]")
        self.assertEqual(entry["attempts"][0]["critique"], "type mismatch")


class RateGovernorTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.path = Path(self.tmpdir.name) / "rate-governor.json"

    def tearDown(self):
        self.tmpdir.cleanup()

    def test_none_path_is_a_noop(self):
        governor_acquire(None)  # must not raise or sleep
        governor_report(None, limited=True)

    def test_burst_tokens_allow_immediate_calls_then_throttle(self):
        clock = [1000.0]
        sleeps = []

        def now():
            return clock[0]

        def sleep(s):
            sleeps.append(s)
            clock[0] += s

        for _ in range(5):  # burst = 5 -> all immediate
            governor_acquire(self.path, calls_per_minute=60, burst=5,
                             now_fn=now, sleep_fn=sleep, jitter_fn=lambda: 0.5)
        self.assertEqual(sleeps, [])
        # 6th call: bucket empty, refill is 1/sec -> must wait ~1s
        governor_acquire(self.path, calls_per_minute=60, burst=5,
                         now_fn=now, sleep_fn=sleep, jitter_fn=lambda: 0.5)
        self.assertEqual(len(sleeps), 1)
        self.assertGreater(sleeps[0], 0)
        self.assertLess(sleeps[0], 2.5)

    def test_report_limited_sets_global_cooldown_acquire_waits_it_out(self):
        clock = [1000.0]
        sleeps = []

        def now():
            return clock[0]

        def sleep(s):
            sleeps.append(s)
            clock[0] += s

        governor_report(self.path, limited=True, cooldown_seconds=30,
                        max_cooldown_seconds=300, now_fn=now)
        governor_acquire(self.path, calls_per_minute=60, burst=5,
                         now_fn=now, sleep_fn=sleep, jitter_fn=lambda: 0.5)
        self.assertTrue(sleeps)
        self.assertGreaterEqual(sum(sleeps), 30 * 0.8)  # jitter can shave 20%

    def test_consecutive_limited_reports_grow_the_cooldown_capped(self):
        now_fn = lambda: 1000.0
        for _ in range(10):
            governor_report(self.path, limited=True, cooldown_seconds=30,
                            max_cooldown_seconds=120, now_fn=now_fn)
        state = json.loads(self.path.read_text())
        self.assertLessEqual(state["cooldown_until"], 1000.0 + 120)
        self.assertGreaterEqual(state["consecutive_limited"], 10)

    def test_success_resets_the_streak(self):
        now_fn = lambda: 1000.0
        governor_report(self.path, limited=True, now_fn=now_fn)
        governor_report(self.path, limited=False, now_fn=now_fn)
        state = json.loads(self.path.read_text())
        self.assertEqual(state["consecutive_limited"], 0)

    def test_corrupt_state_file_recovers_permissively(self):
        self.path.write_text("{not json")
        governor_acquire(self.path, calls_per_minute=60, burst=5,
                         now_fn=lambda: 1000.0, sleep_fn=lambda s: None,
                         jitter_fn=lambda: 0.5)  # must not raise
        governor_report(self.path, limited=False, now_fn=lambda: 1000.0)
        json.loads(self.path.read_text())  # now valid again


if __name__ == "__main__":
    unittest.main()
