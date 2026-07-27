import datetime
import difflib
import email.utils
import io
import json
import multiprocessing
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
from unittest.mock import patch, MagicMock
from pathlib import Path

import model_fix_loop
from model_fix_loop import (
    ARCHITECTURE_PRIMER,
    KNOWN_PITFALLS,
    DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS,
    DEFAULT_TABLE_JOB_MAX_REPAIR_ROUNDS,
    FOUNDATION_JOB_CLAIM_PREFIX,
    TABLE_JOB_CLAIM_PREFIX,
    _dedupe_machine_entries,
    _entry_is_human,
    _extract_test_failure_context,
    _is_rejection_entry,
    _normalize_model_config,
    _quota_exhausted_message,
    _retry_after_seconds,
    _select_tier,
    _state_locked,
    _table_port_pseudo_gap,
    attempt_foundation_job,
    attempt_table_port,
    build_foundation_job_prompt,
    build_table_port_prompt,
    build_table_port_registry_skeleton,
    claim_conflicts,
    claim_foundation_job,
    claim_table_job,
    evaluate_table_port_gate,
    extract_perl_table_source,
    foundation_job_claim_key,
    gather_live_claims,
    load_foundation_jobs,
    DEFAULT_DEADLINE_SECONDS,
    INFRA_FAILURE_PREFIX,
    ModelCallDeadlineExceeded,
    ModelQuotaExhausted,
    load_tag_state,
    mark_held_by_foundation,
    normalize_table_job_config,
    release_foundation_job_claim,
    release_table_job_claim,
    resolve_canonical_table,
    resolve_foundation_job_tag_keys,
    save_tag_state,
    table_job_claim_key,
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
    compose_learning_block,
    count_diff_format_failures,
    build_diff_format_remediation,
    DEFAULT_GOVERNOR_BURST,
    DEFAULT_LEARNING_BUDGET_TOKENS,
    DEFAULT_QUARANTINE_MAX_ENTRIES,
    LEARNING_SECTION_ORDER,
    QUARANTINE_FLAGS_DISPLAY_CHARS,
    QUARANTINE_REASON_DISPLAY_CHARS,
    detect_duplicate_tag_insertion,
    estimate_tokens,
    expand_gaps_to_tags,
    extract_diff,
    extract_perl_table_notes,
    extract_perl_tag_snippet,
    extract_review_verdict,
    file_content_at_head,
    find_implemented_sibling,
    fix_gap,
    format_lessons_tail,
    format_own_quarantine,
    format_previous_attempts,
    load_global_pitfalls,
    load_module_playbook,
    format_sweep_review_history,
    read_lessons_tail_events,
    read_own_quarantine,
    select_module_lessons,
    squad_from_worker,
    describe_missing_path,
    FORCED_DIFF_DEMAND,
    git_apply,
    git_apply_with_rung,
    GIT_APPLY_LADDER,
    git_checkout_clean,
    git_commit,
    render_request_budget_footer,
    governor_acquire,
    governor_report,
    load_landed_tags,
    load_recent_sweep_reviews,
    load_toml_config,
    make_cluster_gap,
    make_single_tag_gap,
    models_for_phase,
    new_oxidex_only_keys,
    newly_duplicated_emissions,
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


class AppendLessonWrapperTests(unittest.TestCase):
    """Spec K1 item 1: model_fix_loop.append_lesson is a thin wrapper
    delegating to distill_lessons.append_lesson (the canonical K1 owner),
    a sibling import exactly like find_tag_gaps."""

    def _event(self):
        import distill_lessons
        return distill_lessons.make_lesson(
            ts=1_784_800_000, worker="w1", format_name="JPEG", module="Canon.pm",
            event="wrong_value", reason="PrintConv must match Perl byte-for-byte",
        )

    def test_home_dir_resolves_to_logs_lessons_jsonl(self):
        from model_fix_loop import append_lesson
        with tempfile.TemporaryDirectory() as tmpdir:
            append_lesson(tmpdir, self._event())
            path = Path(tmpdir) / "logs" / "lessons.jsonl"
            lines = path.read_text().splitlines()
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0])["event"], "wrong_value")

    def test_full_lessons_path_is_used_as_is(self):
        from model_fix_loop import append_lesson
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "some" / "dir" / "lessons.jsonl"
            append_lesson(path, self._event())
            lines = path.read_text().splitlines()
        self.assertEqual(len(lines), 1)

    def test_appends_do_not_clobber(self):
        from model_fix_loop import append_lesson
        with tempfile.TemporaryDirectory() as tmpdir:
            append_lesson(tmpdir, self._event())
            append_lesson(tmpdir, self._event())
            path = Path(tmpdir) / "logs" / "lessons.jsonl"
            self.assertEqual(len(path.read_text().splitlines()), 2)

    def test_fingerprints_match_distill_lessons_directly(self):
        # Byte-identical fingerprints across every K1 writer -- the
        # whole point of the canonical-owner refactor (item 1).
        import distill_lessons
        from model_fix_loop import append_lesson
        with tempfile.TemporaryDirectory() as tmpdir:
            event = self._event()
            append_lesson(tmpdir, event)
        self.assertEqual(
            event["fingerprint_generic"],
            distill_lessons.fingerprint_generic("wrong_value", "", event["reason"]),
        )


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
        self.assertEqual(config["compaction_min_elide_tokens"], 3000)

    def test_new_harness_knobs_are_overridable(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m"],
            "max_request_repeats": 5, "max_verify_turns": 2,
            "compaction_trigger_tokens": 6000, "compaction_keep_recent_turns": 8,
            "compaction_min_elide_tokens": 500,
        })
        self.assertEqual(config["max_request_repeats"], 5)
        self.assertEqual(config["max_verify_turns"], 2)
        self.assertEqual(config["compaction_trigger_tokens"], 6000)
        self.assertEqual(config["compaction_keep_recent_turns"], 8)
        self.assertEqual(config["compaction_min_elide_tokens"], 500)

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

    def test_claim_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["claim_stale_seconds"], 7200)
        self.assertEqual(config["heartbeat_seconds"], 60)

    def test_claim_knobs_are_overridable(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m"],
            "claim_stale_seconds": 3600, "heartbeat_seconds": 30,
        })
        self.assertEqual(config["claim_stale_seconds"], 3600)
        self.assertEqual(config["heartbeat_seconds"], 30)

    def test_build_semaphore_has_a_default(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["build_semaphore"], 5)

    def test_build_semaphore_is_overridable(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m"], "build_semaphore": 10,
        })
        self.assertEqual(config["build_semaphore"], 10)

    def test_section6_k5_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["reviewer_max_prompt_tokens"], 8192)
        self.assertEqual(config["learning_budget_tokens"], 1200)
        self.assertEqual(config["parser_floor_tokens"], 2000)
        self.assertEqual(config["lessons_tail_kb"], 256)

    def test_section6_k5_knobs_are_overridable(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m"],
            "reviewer_max_prompt_tokens": 4096, "learning_budget_tokens": 600,
            "parser_floor_tokens": 1000, "lessons_tail_kb": 64,
        })
        self.assertEqual(config["reviewer_max_prompt_tokens"], 4096)
        self.assertEqual(config["learning_budget_tokens"], 600)
        self.assertEqual(config["parser_floor_tokens"], 1000)
        self.assertEqual(config["lessons_tail_kb"], 64)

    def test_default_max_prompt_tokens_is_8192(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["max_prompt_tokens"], 8192)


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


class ExtractTestFailureContextTests(unittest.TestCase):
    def test_surfaces_panic_pushed_past_tail_window_by_later_binaries(self):
        panic_block = (
            "running 3 tests\n"
            "test jpeg::app12_stb2 ... FAILED\n\n"
            "failures:\n\n"
            "---- jpeg::app12_stb2 stdout ----\n"
            "thread 'jpeg::app12_stb2' panicked at src/parsers/jpeg/app12.rs:88:9:\n"
            "assertion `left == right` failed\n"
            "  left: 3\n"
            " right: 2\n\n"
            "failures:\n"
            "    jpeg::app12_stb2\n\n"
            "test result: FAILED. 2 passed; 1 failed; 0 ignored\n\n"
        )
        # Simulate many unrelated, PASSING test binaries printing after the
        # real failure -- this is what pushed the panic out of any blind
        # tail-keep in the observed live regressions (nikon-1, canon-3, etc).
        noise = "".join(f"running 50 tests\ntest mod{i}::case ... ok\n" * 1 for i in range(2000))
        output = panic_block + noise
        self.assertGreater(len(output), 20000)

        extracted = _extract_test_failure_context(output, max_chars=8000)

        self.assertIn("panicked at src/parsers/jpeg/app12.rs:88:9", extracted)
        self.assertIn("assertion `left == right` failed", extracted)
        self.assertLessEqual(len(extracted), 8000 + 20)

    def test_falls_back_to_blind_tail_when_no_markers_found(self):
        output = "line\n" * 5000
        extracted = _extract_test_failure_context(output, max_chars=100)
        self.assertEqual(extracted, output[-100:])

    def test_empty_output_returns_empty(self):
        self.assertEqual(_extract_test_failure_context("", max_chars=100), "")


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


class AssemblePromptSectionsTests(unittest.TestCase):
    """Section 6: assemble_prompt_sections's own property-style checks --
    total <= cap, parser floor honored, learning block present at 8192
    with a 60 KB parser section (spec Testing section, verbatim)."""

    def test_no_shrink_when_already_under_budget(self):
        from model_fix_loop import assemble_prompt_sections
        sections = [("a", "short"), ("b", "also short")]
        result = assemble_prompt_sections(sections, {"a": 0}, max_tokens=1000)
        self.assertEqual(result, "shortalso short")

    def test_60kb_parser_section_at_8192_leaves_learning_block_present(self):
        from model_fix_loop import assemble_prompt_sections, estimate_tokens
        parser_text = "x" * 60_000
        learning_text = "LEARNING-MARKER: distilled lessons go here"
        sections = [
            ("intro", "short static intro"),
            ("attempts", ""),
            ("samples", ""),
            ("neighbor", ""),
            ("perl_block", ""),
            ("parser_files", parser_text),
            ("learning", learning_text),
            ("tail", "tail text"),
        ]
        budgets = {
            "attempts": 0, "samples": 0, "neighbor": 0, "perl_block": 0,
            "parser_files": 2000,
        }
        result = assemble_prompt_sections(sections, budgets, max_tokens=8192)
        self.assertLessEqual(estimate_tokens(result), 8192 + 10)  # small rounding slack
        self.assertIn("LEARNING-MARKER", result)  # never dropped

    def test_parser_floor_is_never_crossed(self):
        from model_fix_loop import assemble_prompt_sections, estimate_tokens
        sections = [("parser_files", "y" * 100_000)]
        result = assemble_prompt_sections(sections, {"parser_files": 500}, max_tokens=1)
        self.assertGreaterEqual(estimate_tokens(result), 500)

    def test_priority_order_shrinks_earlier_sections_first(self):
        from model_fix_loop import assemble_prompt_sections
        sections = [("first", "a" * 4000), ("second", "b" * 4000)]
        # Total ~2000 tokens; cap 1200 forces ~800 tokens of shrinkage.
        # "first" is listed before "second" in budgets, so it absorbs the
        # overflow first -- "second" (listed later) survives untouched as
        # long as "first" alone can absorb the whole deficit.
        result = assemble_prompt_sections(
            sections, {"first": 0, "second": 0}, max_tokens=1200,
        )
        self.assertLess(result.count("a"), 4000)  # first was shrunk
        self.assertIn("b" * 4000, result)  # second untouched

    def test_sections_not_in_budgets_are_never_shrunk(self):
        from model_fix_loop import assemble_prompt_sections
        sections = [("protected", "p" * 100_000), ("elastic", "e" * 100_000)]
        result = assemble_prompt_sections(sections, {"elastic": 0}, max_tokens=10)
        self.assertIn("p" * 100_000, result)  # untouched even though huge

    def test_shrink_order_comes_from_budgets_not_from_render_order(self):
        """Render order and shrink priority are independent axes, and this
        pins that they stay independent: since 2026-07-26 build_prompt
        renders most-cacheable-first (PROMPT_SECTION_ORDER) while still
        shedding least-essential-first (PROMPT_SHRINK_PRIORITY). Deriving
        one from the other -- which the single `sections` list used to
        invite -- would silently start truncating parser source before
        attempt history."""
        from model_fix_loop import assemble_prompt_sections
        sections = [("rendered_first", "a" * 4000), ("rendered_second", "b" * 4000)]
        # budgets deliberately lists them in the OPPOSITE order.
        result = assemble_prompt_sections(
            sections, {"rendered_second": 0, "rendered_first": 0}, max_tokens=1200,
        )
        self.assertIn("a" * 4000, result)          # rendered first, shrunk last
        self.assertLess(result.count("b"), 4000)   # rendered last, shrunk first
        self.assertLess(result.index("a"), result.index("b"))  # order preserved


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
        # Pin the elide floor below the ~2000-token fixture payloads so this
        # exercises the stubbing mechanic independent of the default floor.
        result = compact_messages(messages, trigger_tokens=100, keep_recent=2,
                                  min_elide_tokens=1000)
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

    def test_min_elide_tokens_sets_the_elide_floor(self):
        messages = self._messages()
        # The served payloads (messages 2 and 4) are ~2000 estimated tokens
        # each. A floor below that stubs them; a floor above leaves them intact.
        low_floor = compact_messages(messages, trigger_tokens=100, keep_recent=2,
                                     min_elide_tokens=1000)
        self.assertIn("[earlier content elided for space:", low_floor[2]["content"])
        high_floor = compact_messages(messages, trigger_tokens=100, keep_recent=2,
                                      min_elide_tokens=3000)
        self.assertEqual(high_floor[2], messages[2])


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


class CallModelDeadlineTests(unittest.TestCase):
    """A slow-drip stream must be killed and replayed.

    urlopen's `timeout` only bounds a single read, and every arriving SSE
    chunk resets it -- so a provider trickling content satisfies it
    indefinitely. Measured on theclawbay.com: one call ran 2118s against a
    configured timeout=1200. deadline_seconds is the wall-clock bound that
    actually cuts that off.
    """

    def _sse(self, *texts):
        return [
            ('data: ' + json.dumps({"choices": [{"delta": {"content": t}}]}) + '\n').encode()
            for t in texts
        ] + [b'data: [DONE]\n']

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_slow_stream_is_abandoned_and_replayed(self, mock_urlopen):
        # A stream that never stalls on any single read (so `timeout` can
        # never fire) but takes 10s of wall clock per chunk.
        slow_cm = MagicMock()
        slow_cm.__iter__.return_value = iter(self._sse("a", "b", "c", "d"))
        slow_ctx = MagicMock()
        slow_ctx.__enter__.return_value = slow_cm

        ok_cm = MagicMock()
        ok_cm.__iter__.return_value = iter(self._sse("recovered"))
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm

        mock_urlopen.side_effect = [slow_ctx, ok_ctx]
        # Attempt 1 starts at t=0 and is cut off at t=30 (past the 25s
        # deadline) partway through the stream; the replay starts at t=40
        # and streams briskly, so it finishes well inside its own window.
        clock = iter([0, 0, 10, 20, 30] + [40, 40, 41, 42, 43] + [44] * 20)
        with patch("model_fix_loop.time.monotonic", lambda: next(clock)):
            reply = call_model(
                [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "low",
                stream=True, timeout=1200, deadline_seconds=25,
                sleep_fn=lambda s: None,
            )
        # First attempt was cut off mid-stream; the replay's reply is returned.
        self.assertEqual(reply, "recovered")
        self.assertEqual(mock_urlopen.call_count, 2)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_deadline_is_reported_to_governor_as_not_rate_limited(self, mock_urlopen):
        # A slow provider is not a rate limit. Reporting it as one would
        # put the whole fleet into a global cooldown over one sluggish
        # call, which is the opposite of what should happen.
        slow_cm = MagicMock()
        slow_cm.__iter__.return_value = iter(self._sse("a", "b", "c"))
        slow_ctx = MagicMock()
        slow_ctx.__enter__.return_value = slow_cm
        ok_cm = MagicMock()
        ok_cm.__iter__.return_value = iter(self._sse("ok"))
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [slow_ctx, ok_ctx]

        with tempfile.TemporaryDirectory() as tmpdir:
            gov = Path(tmpdir) / "gov.json"
            # Attempt 1 blows the 15s deadline at t=20; the replay starts
            # at t=30 and completes promptly.
            clock = iter([0, 0, 10, 20] + [30, 30, 31, 32, 33] + [34] * 20)
            with patch("model_fix_loop.time.monotonic", lambda: next(clock)):
                call_model(
                    [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "low",
                    stream=True, deadline_seconds=15, sleep_fn=lambda s: None,
                    governor_path=gov,
                )
            state = json.loads(gov.read_text())
        self.assertEqual(state["consecutive_limited"], 0)
        self.assertEqual(state["cooldown_until"], 0.0)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_gives_up_after_max_retries_on_persistent_slowness(self, mock_urlopen):
        def always_slow(*_a, **_kw):
            cm = MagicMock()
            cm.__iter__.return_value = iter(self._sse("a", "b", "c"))
            ctx = MagicMock()
            ctx.__enter__.return_value = cm
            return ctx

        mock_urlopen.side_effect = always_slow
        clock = iter(range(0, 100000, 10))
        with patch("model_fix_loop.time.monotonic", lambda: next(clock)):
            with self.assertRaises(ModelCallDeadlineExceeded):
                call_model(
                    [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "low",
                    stream=True, deadline_seconds=5, max_retries=2,
                    sleep_fn=lambda s: None,
                )
        # 1 initial attempt + 2 retries: persistent slowness eventually
        # gives up rather than replaying forever.
        self.assertEqual(mock_urlopen.call_count, 3)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_fast_stream_under_deadline_is_untouched(self, mock_urlopen):
        cm = MagicMock()
        cm.__iter__.return_value = iter(self._sse("hello ", "world"))
        ctx = MagicMock()
        ctx.__enter__.return_value = cm
        mock_urlopen.side_effect = [ctx]
        clock = iter([0, 0, 1, 2, 3, 4, 5, 6])
        with patch("model_fix_loop.time.monotonic", lambda: next(clock)):
            reply = call_model(
                [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "low",
                stream=True, deadline_seconds=120, sleep_fn=lambda s: None,
            )
        self.assertEqual(reply, "hello world")
        self.assertEqual(mock_urlopen.call_count, 1)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_socket_timeout_is_clamped_to_remaining_deadline(self, mock_urlopen):
        # Otherwise a connection that stalls just before the deadline
        # still blocks for the full (much larger) `timeout` first.
        cm = MagicMock()
        cm.read.return_value = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ctx = MagicMock()
        ctx.__enter__.return_value = cm
        mock_urlopen.side_effect = [ctx]
        clock = iter([0, 0, 0, 0, 0])
        with patch("model_fix_loop.time.monotonic", lambda: next(clock)):
            call_model(
                [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "low",
                stream=False, timeout=1200, deadline_seconds=120,
                sleep_fn=lambda s: None,
            )
        self.assertEqual(mock_urlopen.call_args.kwargs["timeout"], 120)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_deadline_none_restores_unbounded_behavior(self, mock_urlopen):
        cm = MagicMock()
        cm.__iter__.return_value = iter(self._sse("a", "b"))
        ctx = MagicMock()
        ctx.__enter__.return_value = cm
        mock_urlopen.side_effect = [ctx]
        with patch("model_fix_loop.time.monotonic", lambda: 10**9):
            reply = call_model(
                [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "low",
                stream=True, deadline_seconds=None, sleep_fn=lambda s: None,
            )
        self.assertEqual(reply, "ab")
        self.assertEqual(mock_urlopen.call_args.kwargs["timeout"], 120)

    def test_config_default_is_120_seconds(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["deadline_seconds"], DEFAULT_DEADLINE_SECONDS)
        self.assertEqual(config["deadline_seconds"], 120)

    def test_config_deadline_is_overridable(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m"], "deadline_seconds": 45,
        })
        self.assertEqual(config["deadline_seconds"], 45)


class RetryAfterTests(unittest.TestCase):
    """A 429's Retry-After is the server stating its own wait window --
    strictly better information than any backoff curve we can guess."""

    def test_parses_delta_seconds(self):
        self.assertEqual(_retry_after_seconds({"Retry-After": "30"}), 30.0)

    def test_parses_http_date(self):
        # 60s in the future relative to the injected clock.
        when = datetime.datetime(2026, 7, 25, 12, 1, 0, tzinfo=datetime.timezone.utc)
        header = email.utils.format_datetime(when)
        now = datetime.datetime(2026, 7, 25, 12, 0, 0, tzinfo=datetime.timezone.utc).timestamp()
        self.assertAlmostEqual(
            _retry_after_seconds({"Retry-After": header}, now_fn=lambda: now), 60.0, places=0
        )

    def test_absent_or_malformed_returns_none(self):
        self.assertIsNone(_retry_after_seconds(None))
        self.assertIsNone(_retry_after_seconds({}))
        self.assertIsNone(_retry_after_seconds({"Retry-After": ""}))
        self.assertIsNone(_retry_after_seconds({"Retry-After": "soon-ish"}))

    def test_past_http_date_clamps_to_zero_not_negative(self):
        past = email.utils.format_datetime(
            datetime.datetime(2020, 1, 1, tzinfo=datetime.timezone.utc)
        )
        self.assertEqual(_retry_after_seconds({"Retry-After": past}), 0.0)

    def test_retry_after_raises_the_cooldown_floor(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            gov = Path(tmpdir) / "gov.json"
            # Exponential backoff would be 5s and the cap 10s; the server
            # asked for 300s, which must win -- capping our own guessing
            # is not a reason to ignore an explicit instruction.
            governor_report(gov, limited=True, cooldown_seconds=5,
                            max_cooldown_seconds=10, now_fn=lambda: 1000.0,
                            retry_after_seconds=300)
            state = json.loads(gov.read_text())
        self.assertEqual(state["cooldown_until"], 1300.0)

    def test_smaller_retry_after_does_not_lower_the_cooldown(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            gov = Path(tmpdir) / "gov.json"
            governor_report(gov, limited=True, cooldown_seconds=60,
                            max_cooldown_seconds=300, now_fn=lambda: 1000.0,
                            retry_after_seconds=5)
            state = json.loads(gov.read_text())
        self.assertEqual(state["cooldown_until"], 1060.0)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_429_retry_after_reaches_the_governor(self, mock_urlopen):
        err = urllib.error.HTTPError(
            url="https://api.example/v1/chat/completions", code=429,
            msg="Too Many Requests", hdrs={"Retry-After": "240"}, fp=None,
        )
        ok_cm = MagicMock()
        ok_cm.read.return_value = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [err, ok_ctx]

        seen = {}

        def fake_report(path, limited, cooldown_seconds=None, max_cooldown_seconds=None,
                        now_fn=None, retry_after_seconds=None):
            if limited:
                seen["retry_after"] = retry_after_seconds

        with tempfile.TemporaryDirectory() as tmpdir:
            gov = Path(tmpdir) / "gov.json"
            with patch("model_fix_loop.governor_report", fake_report):
                call_model(
                    [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "low",
                    sleep_fn=lambda s: None, governor_path=gov,
                )
        self.assertEqual(seen["retry_after"], 240.0)


class QuotaExhaustedTests(unittest.TestCase):
    """A 429 meaning "account out of budget" must fail fast, not ride the
    retry ladder. Providers overload 429 for both throttling and billing;
    only the first is worth waiting out."""

    # Trimmed from a real theclawbay.com 429 captured 2026-07-25.
    REAL_BODY = {
        "error": "weekly cost limit reached for this account",
        "code": "weekly_cost_limit_reached",
        "theclawbayError": {
            "requestId": "acea5cad-24c2-4485-94b3-5f52ca69dc96",
            "category": "quota",
            "code": "weekly_cost_limit_reached",
            "userMessage": "Your weekly The Claw Bay usage limit has been reached.",
            "retryable": False,
        },
    }

    def _http_error(self, code, body):
        return urllib.error.HTTPError(
            url="https://api.example/v1/chat/completions", code=code,
            msg="Too Many Requests", hdrs=None,
            fp=io.BytesIO(json.dumps(body).encode()),
        )

    def test_recognizes_real_clawbay_weekly_limit_body(self):
        msg = _quota_exhausted_message(self._http_error(429, self.REAL_BODY))
        self.assertIsNotNone(msg)
        self.assertIn("weekly", msg.lower())
        self.assertIn("weekly_cost_limit_reached", msg)

    def test_plain_rate_limit_429_is_still_retryable(self):
        body = {"error": {"message": "Rate limit reached for gpt-5.5", "type": "rate_limit_error"}}
        self.assertIsNone(_quota_exhausted_message(self._http_error(429, body)))

    def test_unparseable_body_falls_through_to_retry(self):
        # Wrongly giving up on a transient 429 is worse than wrongly
        # retrying a permanent one, so anything ambiguous stays retryable.
        err = urllib.error.HTTPError(
            url="https://u", code=429, msg="x", hdrs=None, fp=io.BytesIO(b"<html>nope</html>"),
        )
        self.assertIsNone(_quota_exhausted_message(err))

    def test_non_429_is_never_classified_as_quota(self):
        self.assertIsNone(_quota_exhausted_message(self._http_error(500, self.REAL_BODY)))

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_quota_429_is_retried_like_any_other_429(self, mock_urlopen):
        # Policy: ALL 429s retry, quota included. The token bucket's global
        # cooldown is what paces the wait, so the fleet resumes by itself
        # once the billing window rolls over.
        ok_cm = MagicMock()
        ok_cm.read.return_value = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [
            self._http_error(429, self.REAL_BODY),
            self._http_error(429, self.REAL_BODY),
            ok_ctx,
        ]
        reply = call_model(
            [{"role": "user", "content": "x"}], "https://u", "k", "gpt-5.5", 4096, "medium",
            max_retries=10, sleep_fn=lambda s: None,
        )
        self.assertEqual(reply, "hi")
        self.assertEqual(mock_urlopen.call_count, 3)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_quota_429_does_pace_the_fleet_via_the_token_bucket(self, mock_urlopen):
        # Since we now wait rather than bail, the global cooldown must be
        # set -- that is the whole mechanism keeping a quota-blocked fleet
        # from hot-looping against the provider.
        mock_urlopen.side_effect = self._http_error(429, self.REAL_BODY)
        with tempfile.TemporaryDirectory() as tmpdir:
            gov = Path(tmpdir) / "gov.json"
            # cooldown_seconds=0: the quota 429 must still be REPORTED as
            # limited without this test sitting out a real cooldown --
            # governor_acquire busy-waits against the wall clock when
            # sleep_fn is a no-op. The cooldown arithmetic itself is
            # covered directly in RetryAfterTests.
            with self.assertRaises(ModelQuotaExhausted):
                call_model(
                    [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "medium",
                    max_retries=2, sleep_fn=lambda s: None, governor_path=gov,
                    governor_cooldown_seconds=0, governor_max_cooldown_seconds=0,
                )
            state = json.loads(gov.read_text())
        # limited on every attempt -- this streak is what drives the
        # exponential global cooldown in a real run.
        self.assertEqual(state["consecutive_limited"], 3)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_names_billing_as_the_cause_once_retries_are_spent(self, mock_urlopen):
        # A bare "HTTP Error 429" after a long ride-out sends an operator
        # looking at rate limits when the answer is billing.
        mock_urlopen.side_effect = self._http_error(429, self.REAL_BODY)
        with self.assertRaises(ModelQuotaExhausted) as ctx:
            call_model(
                [{"role": "user", "content": "x"}], "https://u", "k", "gpt-5.5", 4096, "medium",
                max_retries=3, sleep_fn=lambda s: None,
            )
        # Every retry was still made -- naming the cause is not a shortcut.
        self.assertEqual(mock_urlopen.call_count, 4)
        self.assertIn("gpt-5.5", str(ctx.exception))
        self.assertIn("weekly", str(ctx.exception).lower())

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_quota_retry_is_logged_distinctly_from_throttling(self, mock_urlopen):
        ok_cm = MagicMock()
        ok_cm.read.return_value = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [self._http_error(429, self.REAL_BODY), ok_ctx]
        logged = []
        call_model(
            [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "medium",
            sleep_fn=lambda s: None, log_fn=logged.append,
        )
        self.assertTrue(any("quota exhausted" in m for m in logged), logged)

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_plain_rate_limit_429_is_not_labelled_as_quota(self, mock_urlopen):
        ok_cm = MagicMock()
        ok_cm.read.return_value = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_ctx = MagicMock()
        ok_ctx.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [
            self._http_error(429, {"error": {"message": "Rate limit reached"}}),
            ok_ctx,
        ]
        logged = []
        call_model(
            [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "medium",
            sleep_fn=lambda s: None, log_fn=logged.append,
        )
        self.assertFalse(any("quota exhausted" in m for m in logged), logged)


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
    def test_first_rung_is_exact_and_nothing_looser_runs_when_it_applies(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stderr="")
        ok, msg = git_apply("diff text", Path("/fake/repo"))
        self.assertTrue(ok)
        self.assertEqual(msg, "applied")
        # Exactly one exec: the ladder must stop at the rung that worked.
        self.assertEqual(mock_run.call_count, 1)
        args, kwargs = mock_run.call_args
        # No --reject: it makes git apply non-atomic, which would leave the
        # next rung matching against a half-patched tree.
        self.assertEqual(args[0], ["git", "apply", "--recount", "-"])
        self.assertNotIn("--reject", args[0])
        self.assertEqual(kwargs["input"], "diff text")
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))

    @patch("model_fix_loop.subprocess.run")
    def test_every_rung_is_tried_before_failing(self, mock_run):
        mock_run.return_value = MagicMock(returncode=1, stderr="patch does not apply\n", stdout="")
        ok, msg, rung = git_apply_with_rung("bad diff", Path("/fake/repo"))
        self.assertFalse(ok)
        self.assertIsNone(rung)
        applies = [c.args[0] for c in mock_run.call_args_list if c.args[0][:2] == ["git", "apply"]]
        self.assertEqual(applies, [argv for _, argv in GIT_APPLY_LADDER])
        # The STRICT rung's stderr is what comes back (it names the context
        # git searched for), plus a note that looser rungs were already tried
        # so the model doesn't waste its retry on a whitespace tweak.
        self.assertTrue(msg.startswith("patch does not apply"))
        self.assertIn("ignore-whitespace", msg)
        self.assertIn("3way", msg)

    @patch("model_fix_loop.subprocess.run")
    def test_reports_which_rung_applied(self, mock_run):
        results = [
            MagicMock(returncode=1, stderr="nope\n", stdout=""),   # exact
            MagicMock(returncode=1, stderr="nope\n", stdout=""),   # ignore-whitespace
            MagicMock(returncode=0, stderr="", stdout=""),         # context1
        ]
        mock_run.side_effect = results
        ok, msg, rung = git_apply_with_rung("drifted diff", Path("/fake/repo"))
        self.assertTrue(ok)
        self.assertEqual(rung, "context1")
        self.assertIn("context1", msg)
        self.assertEqual(mock_run.call_count, 3)


class GitApplyLadderIntegrationTests(unittest.TestCase):
    """The ladder against a real git binary in a tempdir repo -- mocks can't
    tell us whether -C1 actually rescues a drifted hunk, nor whether a failed
    rung leaves the tree clean for the next one (the property the whole
    ladder rests on)."""

    def _make_repo(self, tmpdir, content):
        import subprocess as sp
        env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}
        repo = Path(tmpdir) / "repo"
        repo.mkdir()

        def git(*args, input_text=None, check=True):
            return sp.run(
                ["git", *args], cwd=repo, input=input_text, capture_output=True,
                text=True, check=check, env=env,
            )

        git("init", "-q")
        git("config", "user.email", "fleet@example.com")
        git("config", "user.name", "Fleet Test")
        git("config", "commit.gpgsign", "false")
        (repo / "f.rs").write_text(content)
        git("add", "-A")
        git("commit", "-q", "-m", "base")
        return repo, git

    BASE = "".join(f"line{i}\n" for i in range(1, 21))

    def _model_diff(self, before, after, path="f.rs"):
        """A unified diff of `before` -> `after` in the shape a MODEL emits:
        correct hunk headers, 3 lines of context, and no `index <blob>..`
        line (models never have blob hashes -- which is also why the 3way
        rung can't help them; the tests that need it build their diff with
        `git diff` instead)."""
        return "".join(difflib.unified_diff(
            before.splitlines(keepends=True), after.splitlines(keepends=True),
            fromfile=f"a/{path}", tofile=f"b/{path}",
        ))

    def test_exact_rung_applies_a_clean_diff(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo, _ = self._make_repo(tmpdir, self.BASE)
            diff = self._model_diff(self.BASE, self.BASE.replace("line10\n", "PATCHED\n"))
            ok, msg, rung = git_apply_with_rung(diff, repo)
            content = (repo / "f.rs").read_text()
        self.assertTrue(ok, msg)
        self.assertEqual(rung, "exact")
        self.assertEqual(content.splitlines()[9], "PATCHED")

    def test_indentation_slip_is_rescued_by_ignore_whitespace(self):
        # The classic model slip, measured 2026-07-26: it re-typed the block
        # with tabs where the file uses spaces (2-space-vs-4-space behaves
        # identically). Note the limit of this rung -- a model that drops the
        # indentation ENTIRELY is not rescued here, because that's a
        # whitespace-vs-nothing difference, not a whitespace-amount one.
        on_disk = "".join(f"    line{i}\n" for i in range(1, 21))
        tabbed = "".join(f"\tline{i}\n" for i in range(1, 21))
        with tempfile.TemporaryDirectory() as tmpdir:
            repo, _ = self._make_repo(tmpdir, on_disk)
            diff = self._model_diff(tabbed, tabbed.replace("\tline10\n", "\tPATCHED\n"))
            ok, msg, rung = git_apply_with_rung(diff, repo)
            content = (repo / "f.rs").read_text()
        self.assertTrue(ok, msg)
        self.assertEqual(rung, "ignore-whitespace")
        self.assertIn("PATCHED", content.splitlines()[9])

    def test_context_drift_is_rescued_by_c1(self):
        # The outer context lines moved on (another worker's fix landed
        # there, or the model only ever saw an excerpt); the -/+ lines and
        # the immediate neighbours still match.
        drifted = self.BASE.replace("line8\n", "MOVED_ON_8\n").replace("line12\n", "MOVED_ON_12\n")
        with tempfile.TemporaryDirectory() as tmpdir:
            repo, _ = self._make_repo(tmpdir, drifted)
            diff = self._model_diff(self.BASE, self.BASE.replace("line10\n", "PATCHED\n"))
            ok, msg, rung = git_apply_with_rung(diff, repo)
            content = (repo / "f.rs").read_text()
        self.assertTrue(ok, msg)
        self.assertEqual(rung, "context1")
        # Rescued, not mangled: the change landed exactly where line10 was,
        # and the drifted neighbours are untouched.
        self.assertEqual(content.splitlines()[9], "PATCHED")
        self.assertEqual(content.splitlines()[7], "MOVED_ON_8")
        self.assertEqual(content.splitlines()[11], "MOVED_ON_12")

    def test_unlocatable_change_still_fails_at_every_rung(self):
        # No fuzz anywhere in the ladder: a hunk whose removed line does not
        # exist must NOT be applied somewhere plausible-looking.
        imaginary = "".join(f"imaginary{i}\n" for i in range(1, 21))
        with tempfile.TemporaryDirectory() as tmpdir:
            repo, _ = self._make_repo(tmpdir, self.BASE)
            diff = self._model_diff(imaginary, imaginary.replace("imaginary10\n", "PATCHED\n"))
            ok, msg, rung = git_apply_with_rung(diff, repo)
            content = (repo / "f.rs").read_text()
        self.assertFalse(ok)
        self.assertIsNone(rung)
        self.assertEqual(content, self.BASE)

    def test_failed_rung_leaves_the_tree_byte_identical_for_the_next_one(self):
        # The ladder's core invariant. A 2-file patch that fails on one file
        # must not leave the OTHER file modified -- that was exactly what
        # --reject used to do, and it would make a later rung's "success"
        # mean a doubly-applied edit.
        with tempfile.TemporaryDirectory() as tmpdir:
            repo, git = self._make_repo(tmpdir, self.BASE)
            (repo / "g.rs").write_text(self.BASE)
            git("add", "-A")
            git("commit", "-q", "-m", "add g")
            imaginary = "".join(f"imaginary{i}\n" for i in range(1, 21))
            diff = (
                # g.rs: applies cleanly on its own.
                self._model_diff(self.BASE, self.BASE.replace("line10\n", "PATCHED_G\n"), "g.rs")
                # f.rs: unlocatable, so the patch as a whole must fail.
                + self._model_diff(imaginary, imaginary.replace("imaginary10\n", "PATCHED_F\n"))
            )
            ok, msg, rung = git_apply_with_rung(diff, repo)
            status = git("status", "--porcelain").stdout
            g_content = (repo / "g.rs").read_text()
        self.assertFalse(ok)
        self.assertEqual(status, "", f"tree left dirty by a failed apply: {status!r}")
        self.assertNotIn("PATCHED_G", g_content)

    def test_three_way_finishes_a_half_landed_patch_and_leaves_it_unstaged(self):
        # The one shape measured (2026-07-26) where rungs 1-4 all fail and
        # --3way genuinely rescues: a two-change patch where one change has
        # ALREADY landed. Rungs 1-4 fail as a unit ("patch does not apply")
        # because the second hunk's preimage is gone; the 3-way merge sees
        # ours-already-has-B and applies only A.
        #
        # It must come back UNSTAGED. --3way implies --index, so without
        # _restore_after_three_way the change is staged, and
        # git_checkout_clean's `git checkout -- .` restores the worktree FROM
        # the index -- i.e. a 3way-applied change would survive the revert
        # after a failed build and leak into the next round.
        both = self.BASE.replace("line10\n", "PATCHED_A\n").replace("line18\n", "PATCHED_B\n")
        half = self.BASE.replace("line18\n", "PATCHED_B\n")
        with tempfile.TemporaryDirectory() as tmpdir:
            repo, git = self._make_repo(tmpdir, self.BASE)
            (repo / "f.rs").write_text(both)
            git("add", "-A")
            diff = git("diff", "--cached").stdout   # carries index/blob lines
            git("reset", "-q", "--hard")
            (repo / "f.rs").write_text(half)
            git("add", "-A")
            git("commit", "-q", "-m", "half of it already landed")

            ok, msg, rung = git_apply_with_rung(diff, repo)
            status = git("status", "--porcelain").stdout
            content = (repo / "f.rs").read_text()
            self.assertTrue(ok, msg)
            self.assertEqual(rung, "3way")
            self.assertIn("PATCHED_A", content)
            self.assertIn("PATCHED_B", content)
            # " M" = unstaged modification; "M " (staged) is the bug this pins.
            self.assertTrue(status.startswith(" M"), f"3way left the change staged: {status!r}")
            # And the revert the loop actually performs must undo it.
            git_checkout_clean(repo)
            self.assertEqual(git("status", "--porcelain").stdout, "")
            self.assertNotIn("PATCHED_A", (repo / "f.rs").read_text())

    def test_three_way_conflict_leaves_no_unmerged_entries_behind(self):
        # A conflicting --3way exits 1 having written conflict markers AND an
        # unmerged index entry. git_checkout_clean runs `git checkout -- .`
        # with check=True, which ERRORS on an unmerged path -- i.e. this
        # state used to be able to kill the worker outright.
        with tempfile.TemporaryDirectory() as tmpdir:
            repo, git = self._make_repo(tmpdir, self.BASE)
            (repo / "f.rs").write_text(self.BASE.replace("line10\n", "OURS\n"))
            git("add", "-A")
            diff = git("diff", "--cached").stdout
            git("reset", "-q", "--hard")
            (repo / "f.rs").write_text(self.BASE.replace("line10\n", "THEIRS\n"))
            git("add", "-A")
            git("commit", "-q", "-m", "conflicting change")

            ok, msg, rung = git_apply_with_rung(diff, repo)
            status = git("status", "--porcelain").stdout
            content = (repo / "f.rs").read_text()
            self.assertFalse(ok)
            self.assertNotIn("U", status)
            self.assertNotIn("<<<<<<<", content)
            self.assertEqual(status, "", f"3way conflict left the tree dirty: {status!r}")
            # And the real cleanup path (check=True) must not blow up on it.
            git_checkout_clean(repo)


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

    @patch("model_fix_loop.subprocess.run")
    def test_no_trailers_matches_legacy_call_exactly(self, mock_run):
        # trailers=None (the default) must produce byte-identical argv to
        # every pre-M1 caller -- no extra -m block at all.
        git_commit("fix(nef): wire tags", Path("/fake/repo"), trailers=None)
        commit_calls = [c.args[0] for c in mock_run.call_args_list if c.args[0][1] == "commit"]
        self.assertEqual(commit_calls, [["git", "commit", "-m", "fix(nef): wire tags"]])

    @patch("model_fix_loop.subprocess.run")
    def test_trailers_appended_as_one_extra_dash_m_block(self, mock_run):
        git_commit(
            "fix(nef): wire tags", Path("/fake/repo"),
            trailers=[("Format", "NEF"), ("Tag", "EXIF:LensModel"), ("Tag", "EXIF:ISO")],
        )
        commit_call = next(c.args[0] for c in mock_run.call_args_list if c.args[0][1] == "commit")
        self.assertEqual(commit_call[:4], ["git", "commit", "-m", "fix(nef): wire tags"])
        self.assertEqual(commit_call[4], "-m")
        trailer_block = commit_call[5]
        self.assertEqual(
            trailer_block.splitlines(),
            ["Format: NEF", "Tag: EXIF:LensModel", "Tag: EXIF:ISO"],
        )

    @patch("model_fix_loop.subprocess.run")
    def test_falsy_trailer_values_are_omitted(self, mock_run):
        git_commit(
            "fix(nef): wire tags", Path("/fake/repo"),
            trailers=[("Format", "NEF"), ("Table", None), ("Perl-Ref", "")],
        )
        commit_call = next(c.args[0] for c in mock_run.call_args_list if c.args[0][1] == "commit")
        self.assertEqual(commit_call, ["git", "commit", "-m", "fix(nef): wire tags", "-m", "Format: NEF"])

    def test_dict_trailers_also_work(self):
        with patch("model_fix_loop.subprocess.run") as mock_run:
            git_commit("fix(nef): wire tags", Path("/fake/repo"), trailers={"Format": "NEF"})
        commit_call = next(c.args[0] for c in mock_run.call_args_list if c.args[0][1] == "commit")
        self.assertIn("Format: NEF", commit_call[5])


class SanitizeTrailerValueTests(unittest.TestCase):
    def test_collapses_newlines_and_whitespace(self):
        from model_fix_loop import sanitize_trailer_value
        self.assertEqual(sanitize_trailer_value("a\nb   c\t d"), "a b c d")

    def test_truncates_to_max_chars(self):
        from model_fix_loop import sanitize_trailer_value
        value = sanitize_trailer_value("x" * 300, max_chars=200)
        self.assertEqual(len(value), 200)
        self.assertTrue(value.endswith("…"))


class GitCommitTrailerRoundTripTests(unittest.TestCase):
    """Spec M1: a real git tempdir repo proves git_commit's trailer
    output is exactly what validate_fix_commit.py's parser expects --
    the 'integration-style test in a git tempdir proving the round-trip'
    the spec's Testing section asks for."""

    def _make_repo(self, tmpdir):
        import subprocess as sp
        env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}
        repo = Path(tmpdir) / "repo"
        repo.mkdir()

        def git(*args, input_text=None):
            return sp.run(
                ["git", *args], cwd=repo, input=input_text, capture_output=True,
                text=True, check=True, env=env,
            ).stdout

        git("init", "-q")
        git("config", "user.email", "fleet@example.com")
        git("config", "user.name", "Fleet Test")
        git("config", "commit.gpgsign", "false")
        (repo / "README.md").write_text("base\n")
        git("add", "-A")
        git("commit", "-q", "-m", "base commit")
        return repo, git

    def test_round_trips_through_validate_fix_commit(self):
        import tempfile as tf
        from validate_fix_commit import parse_trailers, validate_commit

        with tf.TemporaryDirectory() as tmpdir:
            repo, git = self._make_repo(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "canon.rs").write_text("pub fn noop() {}\n")
            git_commit(
                "fix(jpeg): wire AELButton", repo,
                trailers=[
                    ("Format", "JPEG"),
                    ("Tag", "MakerNotes:AELButton"),
                    ("Sample", "/samples/canon1.jpg"),
                    ("Exiftool-Value", "On"),
                    ("Oxidex-Value", "On"),
                    ("Perl-Ref", "Canon.pm"),
                    ("Verified", "recheck-pass gaps=1->0"),
                    ("Worker", "canon-1"),
                    ("Table", "Canon::CameraSettings"),
                ],
            )
            sha = git("rev-parse", "HEAD").strip()
            message = git("show", "-s", "--format=%B", sha)
            trailers = parse_trailers(message, repo)
            result = validate_commit(sha, repo)

        self.assertEqual(trailers["Format"], ["JPEG"])
        self.assertEqual(trailers["Tag"], ["MakerNotes:AELButton"])
        self.assertEqual(trailers["Worker"], ["canon-1"])
        self.assertEqual(trailers["Table"], ["Canon::CameraSettings"])
        self.assertEqual(result["checks"]["trailers"], "pass")
        self.assertTrue(result["ok"])

    def test_review_unverifiable_trailer_round_trips(self):
        import tempfile as tf
        from validate_fix_commit import parse_trailers

        with tf.TemporaryDirectory() as tmpdir:
            repo, git = self._make_repo(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "canon.rs").write_text("pub fn noop() {}\n")
            git_commit(
                "fix(jpeg): wire AELButton", repo,
                trailers=[("Format", "JPEG"), ("Tag", "MakerNotes:AELButton"),
                          ("Verified", "recheck-pass gaps=1->0"),
                          ("Review-Unverifiable", "UNVERIFIABLE:C1")],
            )
            sha = git("rev-parse", "HEAD").strip()
            message = git("show", "-s", "--format=%B", sha)
            trailers = parse_trailers(message, repo)
        self.assertEqual(trailers["Review-Unverifiable"], ["UNVERIFIABLE:C1"])


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


class CargoBuildSemaphoreWiringTests(unittest.TestCase):
    """Spec section 5: every cargo build/check/test call site here is
    wrapped in the shared build_semaphore, opt-in via semaphore_path
    (None -- the default -- keeps every existing caller ungated, exactly
    as before this feature existed)."""

    @patch("model_fix_loop.subprocess.run")
    def test_default_semaphore_path_none_is_ungated(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
        # Must not touch any lock file -- no semaphore_path given.
        cargo_build(Path("/fake/repo"))
        cargo_check(Path("/fake/repo"))
        cargo_test_workspace(Path("/fake/repo"))
        cargo_test_targeted(Path("/fake/repo"), "jpeg")
        self.assertEqual(mock_run.call_count, 4)

    @patch("model_fix_loop.subprocess.run")
    def test_semaphore_path_given_holds_a_slot_during_the_call(self, mock_run):
        with tempfile.TemporaryDirectory() as tmpdir:
            sem_path = Path(tmpdir) / "sem.json"
            observed = {}

            def fake_run(*args, **kwargs):
                observed["holders"] = json.loads(sem_path.read_text())["holders"]
                return MagicMock(returncode=0, stdout="", stderr="")

            mock_run.side_effect = fake_run
            cargo_test_targeted(Path("/fake/repo"), "jpeg", semaphore_path=sem_path, semaphore_max_holders=1)
            self.assertEqual(len(observed["holders"]), 1)
            self.assertEqual(json.loads(sem_path.read_text())["holders"], {})


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

    def test_a_tiny_token_budget_shrinks_the_huge_parser_file_to_its_floor(self):
        # Section 6: graduated per-section truncation replaced plain
        # head-keeping -- a budget far below the huge parser-file
        # section's own size still only ever shrinks it down to (never
        # below) parser_floor_tokens, rather than blindly chopping the
        # whole assembled prompt at some byte offset.
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "big.rs").write_text("x" * 100_000)
            gap = {
                "format": "JPEG", "missing_tags": [], "value_differences": [],
                "gap_count": 0, "parser_files": ["big.rs"],
            }
            prompt = build_prompt(
                gap, repo_root=tmp, max_tags=40, max_file_bytes=200_000, max_prompt_tokens=50,
                parser_floor_tokens=200,
            )
        # Far short of the untruncated prompt (100,000-char file alone) --
        # the exact length includes the static architecture/manifest text
        # this loop's own default cap doesn't shrink, so this checks
        # order-of-magnitude truncation happened, not an exact byte count.
        self.assertLess(len(prompt), 10_000)
        # But never below the floor -- big.rs's own marker still shows,
        # proving the parser section wasn't squeezed to nothing.
        self.assertIn("big.rs", prompt)

    def test_parser_floor_is_never_crossed_even_far_under_budget(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "big.rs").write_text("x" * 100_000)
            gap = {
                "format": "JPEG", "missing_tags": [], "value_differences": [],
                "gap_count": 0, "parser_files": ["big.rs"],
            }
            prompt = build_prompt(
                gap, repo_root=tmp, max_tags=40, max_file_bytes=200_000,
                max_prompt_tokens=1, parser_floor_tokens=200,
            )
        # Even a 1-token budget (impossible to honor given the static
        # constraints/manifest text alone) leaves the parser section at
        # least at its floor -- never squeezed to zero.
        self.assertGreaterEqual(estimate_tokens(prompt), 200)


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
                       ":40-120", ":400-", ":-120", ":400",
                       "roughly 4096 tokens", "ephemeral"):
            self.assertIn(needle, manifest)


# --- prompt-cache prefix ordering ------------------------------------------
#
# The fleet's worker pool leads with deepseek/deepseek-v4-pro, whose cache
# READ price is $0.0036/M against $0.435/M for fresh input -- 120x. It is an
# AUTOMATIC PREFIX cache: only a byte-identical LEADING run of a previous
# request is discounted, so section render order is the entire lever (see
# model_fix_loop.PROMPT_SECTION_ORDER for the measurements behind the order
# these tests pin). Measured offline over 108 prompts rebuilt from real
# saved fixer requests across 22 formats: the pre-2026-07-26 order left
# 40.1% of a prompt cacheable against a sibling prompt for the same format
# (17.0% at max_prompt_tokens=32768), the pinned order leaves 89.5% (85.1%).
# Every test below fails on the old order.

def _cache_order_gap(fmt, tag_name, parser_file):
    return {
        "format": fmt,
        "missing_tags": [{"family": "EXIF", "name": tag_name, "value": "v",
                          "tag_id": None, "source_file": None}],
        "value_differences": [],
        "gap_count": 1,
        "parser_files": [parser_file],
    }


class PromptCachePrefixOrderTests(unittest.TestCase):
    """Pins the render order build_prompt uses for provider prefix caching,
    and the invariants that make it pay off. Hermetic: every optional
    section is fed from a tempdir, nothing reads the real OXIDEX_HOME."""

    #: The order as measured-and-shipped on 2026-07-26. A change here is a
    #: change to the fleet's cache-hit rate and its bill; re-run the offline
    #: prefix harness before editing it.
    EXPECTED_ORDER = (
        "invariants", "format_intro", "samples", "parser_files", "overview",
        "learning", "exact_sample", "gaps", "perl_block", "neighbor",
        "attempts", "tail",
    )

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        (self.tmp / "src").mkdir()
        (self.tmp / "src" / "nef.rs").write_text("// PARSER-SOURCE-MARKER\n" + "x" * 500)
        (self.tmp / "src" / "cr2.rs").write_text("// OTHER-PARSER-MARKER\n" + "y" * 500)
        self.samples = self.tmp / "samples"
        self.samples.mkdir()
        (self.samples / "a.nef").write_bytes(b"\x01\x02\x03NEFSAMPLE")
        (self.samples / "a.cr2").write_bytes(b"\x04\x05\x06CR2SAMPLE")
        # knowledge_home: only the module playbook, so the learning block
        # is non-empty and deterministic (no lessons/quarantine ledgers).
        modules = self.tmp / "home" / "logs" / "knowledge" / "modules"
        modules.mkdir(parents=True)
        (modules / "NEF.md").write_text("PLAYBOOK-MARKER for NEF")
        (modules / "CR2.md").write_text("PLAYBOOK-MARKER for CR2")

    def _prompt(self, fmt="NEF", tag_name="LensModel", parser="src/nef.rs",
                sample="a.nef", **kw):
        gap = _cache_order_gap(fmt, tag_name, parser)
        gap["missing_tags"][0]["source_file"] = str(self.samples / sample)
        kw.setdefault("neighbor_precedent_block", "\n\nNEIGHBOR-MARKER precedent")
        return build_prompt(
            gap, repo_root=self.tmp, max_tags=1, samples_dir=self.samples,
            knowledge_home=self.tmp / "home", module_name=fmt,
            max_prompt_tokens=100_000, **kw,
        )

    def test_render_order_constant_is_the_measured_stability_ranking(self):
        self.assertEqual(model_fix_loop.PROMPT_SECTION_ORDER, self.EXPECTED_ORDER)

    def test_rendered_prompt_follows_the_pinned_section_order(self):
        prompt = self._prompt(previous_attempts=[
            {"diff": "--- a/x\n", "status": "failed", "reason": "ATTEMPT-MARKER"},
        ])
        markers = [
            ("invariants", "CRITICAL RUST ARCHITECTURE CONSTRAINTS"),
            ("invariants/pitfalls", "Lessons from mistakes"),
            ("invariants/manifest", "exactly one of these four shapes"),
            ("invariants/primer", "How oxidex is structured, for orientation"),
            ("format_intro", 'format "NEF"'),
            ("samples", "Real sample files available for this format"),
            ("parser_files", "PARSER-SOURCE-MARKER"),
            ("learning", "PLAYBOOK-MARKER"),
            ("exact_sample", "Real sample file containing the tag targeted below"),
            ("gaps", "Missing entirely"),
            ("gaps/diffs", "Value differences"),
            ("neighbor", "NEIGHBOR-MARKER"),
            ("attempts", "ATTEMPT-MARKER"),
            ("tail", TERMINAL_REMINDER),
        ]
        positions = []
        for name, needle in markers:
            self.assertIn(needle, prompt, f"{name} section missing from prompt")
            positions.append((name, prompt.index(needle)))
        self.assertEqual(
            positions, sorted(positions, key=lambda p: p[1]),
            f"sections rendered out of order: {positions}",
        )

    def test_every_static_section_precedes_the_gap_list(self):
        """The whole point of the reorder: the most volatile string in the
        prompt used to sit at byte ~1871, in front of every large static
        block, so all of them were re-billed at full price every call."""
        prompt = self._prompt()
        gap_pos = prompt.index("Missing entirely")
        for needle in ("How oxidex is structured, for orientation",
                       "Real sample files available for this format",
                       "Likely relevant source files",
                       "PARSER-SOURCE-MARKER",
                       "PLAYBOOK-MARKER"):
            self.assertLess(prompt.index(needle), gap_pos, needle)

    def test_different_formats_still_share_the_whole_invariant_block(self):
        """Tier 0 is shared fleet-wide, not just within one worker: a NEF
        worker's prompt and a CR2 worker's prompt agree byte-for-byte
        through the constraints, pitfalls, manifest and primer, and diverge
        only at the format line."""
        a = self._prompt(fmt="NEF", parser="src/nef.rs", sample="a.nef")
        b = self._prompt(fmt="CR2", tag_name="Aperture", parser="src/cr2.rs",
                         sample="a.cr2")
        shared = os.path.commonprefix([a, b])
        for needle in ("CRITICAL RUST ARCHITECTURE CONSTRAINTS",
                       "Lessons from mistakes",
                       "exactly one of these four shapes",
                       "How oxidex is structured, for orientation"):
            self.assertIn(needle, shared, needle)
        # ...and nothing format-specific sneaked in ahead of the split.
        self.assertNotIn("NEF", shared)
        self.assertLess(len(shared) - len(shared.rstrip()), 5)

    def test_same_format_different_tag_shares_the_parser_source(self):
        """The 28 KB of parser source is the single biggest block in the
        prompt; two different tags of one format must still share it."""
        a = self._prompt(tag_name="LensModel")
        b = self._prompt(tag_name="FocalLength")
        shared = os.path.commonprefix([a, b])
        self.assertIn("PARSER-SOURCE-MARKER", shared)
        self.assertIn("PLAYBOOK-MARKER", shared)
        self.assertGreater(len(shared) / len(a), 0.8)

    def test_exact_sample_block_points_forward_at_the_gap_list(self):
        """It renders ABOVE the gap list now (sibling tags share a sample
        file, so it is the more stable of the two), which only reads
        correctly because the lead-in no longer says "this exact tag"."""
        prompt = self._prompt()
        self.assertIn("Real sample file containing the tag targeted below", prompt)
        self.assertNotIn("Real sample file containing this exact tag", prompt)
        self.assertLess(
            prompt.index("Real sample file containing the tag targeted below"),
            prompt.index("Missing entirely"),
        )

    def test_per_tag_reference_blocks_stay_below_the_gap_list(self):
        """perl_block and neighbor keep pointing BACK at the gap list, so
        they must not be hoisted into an earlier tier."""
        prompt = self._prompt()
        self.assertGreater(prompt.index("NEIGHBOR-MARKER"),
                           prompt.index("Missing entirely"))

    def test_shrink_priority_is_not_the_render_order(self):
        """PROMPT_SHRINK_PRIORITY ranks what a fixer can afford to LOSE;
        PROMPT_SECTION_ORDER ranks what caches best. Conflating them would
        start shedding parser source before attempt history."""
        self.assertEqual(
            model_fix_loop.PROMPT_SHRINK_PRIORITY,
            ("attempts", "samples", "neighbor", "perl_block", "parser_files"),
        )
        rendered = [n for n in model_fix_loop.PROMPT_SECTION_ORDER
                    if n in model_fix_loop.PROMPT_SHRINK_PRIORITY]
        self.assertNotEqual(tuple(rendered), model_fix_loop.PROMPT_SHRINK_PRIORITY)

    def test_build_prompt_passes_budgets_in_shrink_priority_order(self):
        captured = {}
        real = model_fix_loop.assemble_prompt_sections

        def spy(sections, budgets, max_tokens):
            captured["sections"] = [n for n, _ in sections]
            captured["budgets"] = list(budgets)
            return real(sections, budgets, max_tokens)

        with patch.object(model_fix_loop, "assemble_prompt_sections", spy):
            self._prompt()
        self.assertEqual(tuple(captured["sections"]), self.EXPECTED_ORDER)
        self.assertEqual(tuple(captured["budgets"]),
                         model_fix_loop.PROMPT_SHRINK_PRIORITY)

    def test_attempts_are_still_shed_before_parser_source(self):
        """Behaviour under budget pressure is unchanged by the reorder."""
        gap = _cache_order_gap("NEF", "LensModel", "src/nef.rs")
        (self.tmp / "src" / "nef.rs").write_text("// PARSER-SOURCE-MARKER\n" + "x" * 40_000)
        prompt = build_prompt(
            gap, repo_root=self.tmp, max_tags=1, max_file_bytes=200_000,
            max_prompt_tokens=3000, parser_floor_tokens=2000,
            previous_attempts=[{"diff": "--- a/x\n", "status": "failed",
                                "reason": "ATTEMPT-MARKER"}],
        )
        self.assertNotIn("ATTEMPT-MARKER", prompt)   # shed first
        self.assertIn("PARSER-SOURCE-MARKER", prompt)  # floored, never emptied

    def test_prompt_bytes_do_not_depend_on_hash_seed(self):
        """Any set iteration, dict ordering, timestamp or random id anywhere
        in the prefix destroys caching for every request behind it. Two
        interpreters with different PYTHONHASHSEED must produce the same
        bytes."""
        script = self.tmp / "render.py"
        script.write_text(
            "import hashlib, sys\n"
            f"sys.path.insert(0, {str(Path(model_fix_loop.__file__).parent)!r})\n"
            "from pathlib import Path\n"
            "import model_fix_loop as m\n"
            f"tmp = Path({str(self.tmp)!r})\n"
            "gap = {'format': 'NEF', 'gap_count': 1, 'parser_files': ['src/nef.rs'],\n"
            "       'value_differences': [],\n"
            "       'missing_tags': [{'family': 'EXIF', 'name': 'LensModel', 'value': 'v',\n"
            "                         'tag_id': None,\n"
            "                         'source_file': str(tmp / 'samples' / 'a.nef')}]}\n"
            "p = m.build_prompt(gap, repo_root=tmp, max_tags=1,\n"
            "                   samples_dir=tmp / 'samples',\n"
            "                   knowledge_home=tmp / 'home', module_name='NEF',\n"
            "                   worker_label='w-1', max_prompt_tokens=100000)\n"
            "sys.stdout.write(hashlib.sha256(p.encode()).hexdigest())\n"
        )
        digests = set()
        for seed in ("0", "1", "12345"):
            env = dict(os.environ, PYTHONHASHSEED=seed)
            out = subprocess.run(  # nosec B603 -- fixed argv, tempdir script
                [sys.executable, str(script)], capture_output=True, text=True,
                env=env, timeout=120, check=True,
            )
            digests.add(out.stdout.strip())
        self.assertEqual(len(digests), 1, f"non-deterministic prompt: {digests}")
        self.assertRegex(digests.pop(), r"^[0-9a-f]{64}$")  # a prompt was really built


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

    # --- open-ended and bare-line forms ------------------------------------
    #
    # The RW2 transcript (deepseek-v4-pro, 2026-07-26T21:23) opened with
    # "REQUEST: src/parsers/xmp/rdf_parser.rs:400-" and got a
    # could-not-resolve rejection, because the START-END-only regex left
    # ":400-" glued to the filename.

    def test_open_ended_range_means_to_end_of_file(self):
        self.assertEqual(parse_request_range("src/x.rs:400-"), ("src/x.rs", 400, None))

    def test_open_start_range_means_from_line_one(self):
        self.assertEqual(parse_request_range("src/x.rs:-120"), ("src/x.rs", 1, 120))

    def test_bare_line_number_becomes_a_window_around_it(self):
        path, start, end = parse_request_range("src/x.rs:400")
        self.assertEqual(path, "src/x.rs")
        self.assertEqual(start, 400 - model_fix_loop.REQUEST_SINGLE_LINE_CONTEXT_BEFORE)
        self.assertEqual(end, 400 + model_fix_loop.REQUEST_SINGLE_LINE_CONTEXT_AFTER)

    def test_bare_line_number_window_is_clamped_at_the_top_of_the_file(self):
        self.assertEqual(parse_request_range("src/x.rs:3")[1], 1)

    def test_zero_forms_strip_the_suffix_and_fall_back_to_whole_file(self):
        # Same forgiving degrade-to-whole-file rule the START-END form has
        # had: a typo'd range must not fail the whole request.
        for typo in ("src/x.rs:0-", "src/x.rs:-0", "src/x.rs:0"):
            self.assertEqual(parse_request_range(typo), ("src/x.rs", None, None), typo)

    def test_non_numeric_open_forms_stay_part_of_the_path(self):
        for shape in ("src/x.rs:abc-", "src/x.rs:-abc", "src/x.rs:abc"):
            self.assertEqual(parse_request_range(shape), (shape, None, None), shape)


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

    def test_open_ended_range_serves_through_end_of_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:97-", repo, None)
        self.assertIn("Lines 97-100 of src/big.rs", answer)
        self.assertIn("97: line97", answer)
        self.assertIn("100: line100", answer)
        self.assertNotIn("96: line96", answer)

    def test_open_ended_range_past_eof_says_so_with_readable_bounds(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:500-", repo, None)
        self.assertIn("only 100 lines", answer)
        self.assertIn("500-EOF", answer)   # not "500-None"

    def test_open_start_range_serves_from_line_one(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:-3", repo, None)
        self.assertIn("Lines 1-3 of src/big.rs", answer)
        self.assertIn("1: line1", answer)
        self.assertNotIn("4: line4", answer)

    def test_bare_line_number_serves_a_window_around_it(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:50", repo, None)
        self.assertIn("30: line30", answer)   # 50 - CONTEXT_BEFORE
        self.assertIn("50: line50", answer)
        self.assertIn("100: line100", answer)  # window clamped to EOF
        self.assertNotIn("29: line29", answer)


class ResolveRequestRejectionTests(unittest.TestCase):
    """A could-not-resolve rejection has to be self-correcting.

    In the RW2 transcript (2026-07-26T21:23) the model asked for
    src/parsers/xmp/artwork_parser.rs -- a plausible name that does not
    exist -- and the rejection told it to "try a path from the list shown"
    while showing it nothing; the prompt's file list was thousands of tokens
    back in a ~17K-token conversation. It then guessed again."""

    def _make_repo(self, tmpdir):
        repo = Path(tmpdir)
        xmp = repo / "src" / "parsers" / "xmp"
        xmp.mkdir(parents=True)
        for name in ("history_parser.rs", "mod.rs", "namespace_mapping.rs",
                     "namespace_resolver.rs", "rdf_parser.rs"):
            (xmp / name).write_text("// stub\n")
        (xmp / "namespaces").mkdir()
        return repo

    def test_lists_the_real_siblings_when_the_parent_exists(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/parsers/xmp/artwork_parser.rs", repo, None)
        self.assertIn("Could not resolve", answer)
        self.assertIn("src/parsers/xmp/ actually contains", answer)
        for name in ("history_parser.rs", "mod.rs", "namespace_mapping.rs",
                     "namespace_resolver.rs", "rdf_parser.rs"):
            self.assertIn(name, answer)
        self.assertIn("namespaces/", answer)   # directories marked as such

    def test_offers_a_did_you_mean_for_a_near_miss(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/parsers/xmp/rdf_parsr.rs", repo, None)
        self.assertIn("Did you mean", answer)
        self.assertIn("src/parsers/xmp/rdf_parser.rs", answer)

    def test_walks_up_to_the_nearest_existing_ancestor(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/parsers/nosuchdir/deeper/x.rs", repo, None)
        self.assertIn("src/parsers/ actually contains", answer)
        self.assertIn("xmp/", answer)

    def test_listing_is_bounded_and_reports_the_overflow(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src").mkdir()
            for i in range(60):
                (repo / "src" / f"p{i:02d}.rs").write_text("// stub\n")
            answer = resolve_request("src/missing.rs", repo, None)
        self.assertIn("(+20 more)", answer)
        self.assertIn("p00.rs", answer)
        self.assertNotIn("p59.rs", answer)

    def test_a_range_suffix_does_not_leak_into_the_rejection(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/parsers/xmp/artwork_parser.rs:400-", repo, None)
        self.assertIn("'src/parsers/xmp/artwork_parser.rs'", answer)
        self.assertNotIn(":400-", answer)

    def test_traversal_out_of_the_roots_lists_nothing(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("../../../etc/passwd", repo, None)
        self.assertIn("Could not resolve", answer)
        self.assertNotIn("actually contains", answer)

    def test_samples_dir_entries_are_listed_too(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            samples = Path(tmpdir) / "samples"
            samples.mkdir()
            (samples / "XMP5.xmp").write_bytes(b"<x/>")
            answer = resolve_request("XMP9.xmp", Path(tmpdir) / "nonexistent-repo", samples)
        self.assertIn("XMP5.xmp", answer)

    def test_empty_listing_falls_back_to_the_bare_rejection(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir) / "does-not-exist"
            answer = describe_missing_path("src/x.rs", repo, None)
        self.assertEqual(answer, "")


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

    # -- Spec K4 two-tier selection: cross-format rejections tier --

    def test_cross_format_rejections_are_included_and_tagged(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entries = [
                {"format": "JPEG", "tag": "A", "verdict": "rejected", "reason": "wrong table"},
            ]
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(log_path, "NEF")
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["format"], "JPEG")
        self.assertEqual(result[0]["_sweep_review_tier"], "other_format")

    def test_cross_format_non_rejections_are_excluded(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entries = [
                {"format": "JPEG", "tag": "A", "verdict": "accepted", "reason": "fine"},
            ]
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(log_path, "NEF")
        self.assertEqual(result, [])

    def test_cross_format_recognizes_verdict_class_rejections_without_legacy_verdict(self):
        # No legacy "verdict" field at all -- only verdict_class -- to
        # isolate _is_rejection_entry's verdict_class branch from its
        # legacy-verdict branch.
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entries = [
                {"format": "JPEG", "tag": "A",
                 "verdict_class": "human_rejected", "reason": "human said no"},
                {"format": "PNG", "tag": "B",
                 "verdict_class": "machine_rejected", "reason": "recheck failed"},
                {"format": "TIFF", "tag": "C",
                 "verdict_class": "machine_accepted", "reason": "shipped"},
            ]
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(log_path, "NEF")
        self.assertEqual({e["tag"] for e in result}, {"A", "B"})

    def test_cross_format_tier_capped_by_max_other_format_entries(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entries = [
                {"format": "JPEG", "tag": f"T{i}", "verdict": "rejected", "reason": f"r{i}",
                 "patch_id": f"p{i}"}
                for i in range(10)
            ]
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(
                log_path, "NEF", max_entries=4, max_other_format_entries=2
            )
        self.assertEqual(len(result), 2)
        self.assertEqual([e["tag"] for e in result], ["T9", "T8"])

    def test_same_format_and_cross_format_tiers_are_independent(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entries = (
                [{"format": "NEF", "tag": f"S{i}", "verdict": "accepted", "reason": "r",
                  "patch_id": f"s{i}"} for i in range(6)]
                + [{"format": "JPEG", "tag": f"X{i}", "verdict": "rejected", "reason": "r",
                    "patch_id": f"x{i}"} for i in range(6)]
            )
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(
                log_path, "NEF", max_entries=2, max_other_format_entries=3
            )
        same = [e for e in result if e.get("_sweep_review_tier") != "other_format"]
        other = [e for e in result if e.get("_sweep_review_tier") == "other_format"]
        self.assertEqual(len(same), 2)
        self.assertEqual(len(other), 3)

    def test_human_verdicts_preferred_over_machine_within_a_tier(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            # Append order (oldest first): human1 is the OLDEST entry,
            # machine1/machine2 are newer. A plain recency cutoff at
            # max_entries=1 would pick machine2; human preference must
            # override that and pick human1 instead.
            entries = [
                {"format": "NEF", "tag": "human1", "verdict": "rejected",
                 "verdict_class": "human_rejected", "reason": "human reason"},
                {"format": "NEF", "tag": "machine1", "verdict": "accepted",
                 "verdict_class": "machine_accepted", "reason": "r", "patch_id": "m1"},
                {"format": "NEF", "tag": "machine2", "verdict": "accepted",
                 "verdict_class": "machine_accepted", "reason": "r2", "patch_id": "m2"},
            ]
            log_path.write_text("\n".join(json.dumps(e) for e in entries) + "\n")
            result = load_recent_sweep_reviews(log_path, "NEF", max_entries=1)
        self.assertEqual([e["tag"] for e in result], ["human1"])


class FormatSweepReviewHistoryTests(unittest.TestCase):
    def test_empty_entries_returns_empty_string(self):
        self.assertEqual(format_sweep_review_history([]), "")

    def test_renders_verdict_tag_and_reason(self):
        rendered = format_sweep_review_history([
            {"format": "NEF", "tag": "ExifIFD:CFAPattern", "verdict": "rejected", "reason": "wrong name"},
        ])
        self.assertIn("REJECTED ExifIFD:CFAPattern: wrong name", rendered)

    def test_same_format_entries_render_under_original_heading_without_format_prefix(self):
        rendered = format_sweep_review_history([
            {"format": "NEF", "tag": "A", "verdict": "accepted", "reason": "r1"},
        ])
        self.assertIn("Recent sweep-review outcomes for this format", rendered)
        self.assertIn("ACCEPTED A: r1", rendered)
        self.assertNotIn("NEF:A", rendered)

    def test_cross_format_entries_render_under_their_own_subheader_with_format_prefix(self):
        rendered = format_sweep_review_history([
            {"format": "JPEG", "tag": "B", "verdict": "rejected", "reason": "r2",
             "_sweep_review_tier": "other_format"},
        ])
        self.assertIn("Rejections from other formats (the mistakes generalize):", rendered)
        self.assertIn("REJECTED JPEG:B: r2", rendered)
        # No same-format entries -- that heading must not appear.
        self.assertNotIn("Recent sweep-review outcomes for this format", rendered)

    def test_combines_both_sections_when_both_tiers_present(self):
        rendered = format_sweep_review_history([
            {"format": "NEF", "tag": "A", "verdict": "accepted", "reason": "r1"},
            {"format": "JPEG", "tag": "B", "verdict": "rejected", "reason": "r2",
             "_sweep_review_tier": "other_format"},
        ])
        self.assertIn("Recent sweep-review outcomes for this format", rendered)
        self.assertIn("Rejections from other formats (the mistakes generalize):", rendered)
        self.assertIn("ACCEPTED A: r1", rendered)
        self.assertIn("REJECTED JPEG:B: r2", rendered)


class IsRejectionEntryTests(unittest.TestCase):
    def test_legacy_verdict_rejected_is_a_rejection(self):
        self.assertTrue(_is_rejection_entry({"verdict": "rejected"}))

    def test_legacy_verdict_accepted_is_not_a_rejection(self):
        self.assertFalse(_is_rejection_entry({"verdict": "accepted"}))

    def test_verdict_class_human_rejected_is_a_rejection(self):
        self.assertTrue(_is_rejection_entry({"verdict_class": "human_rejected"}))

    def test_verdict_class_machine_rejected_is_a_rejection(self):
        self.assertTrue(_is_rejection_entry({"verdict_class": "machine_rejected"}))

    def test_verdict_class_machine_accepted_is_not_a_rejection(self):
        self.assertFalse(_is_rejection_entry({"verdict_class": "machine_accepted"}))

    def test_empty_entry_is_not_a_rejection(self):
        self.assertFalse(_is_rejection_entry({}))


class EntryIsHumanTests(unittest.TestCase):
    def test_missing_verdict_class_counts_as_human_legacy_entry(self):
        self.assertTrue(_entry_is_human({"verdict": "accepted"}))

    def test_human_accepted_is_human(self):
        self.assertTrue(_entry_is_human({"verdict_class": "human_accepted"}))

    def test_human_rejected_is_human(self):
        self.assertTrue(_entry_is_human({"verdict_class": "human_rejected"}))

    def test_machine_accepted_is_not_human(self):
        self.assertFalse(_entry_is_human({"verdict_class": "machine_accepted"}))

    def test_machine_rejected_is_not_human(self):
        self.assertFalse(_entry_is_human({"verdict_class": "machine_rejected"}))


class DedupeMachineEntriesTests(unittest.TestCase):
    def test_drops_machine_entry_sharing_patch_id_and_reason(self):
        entries = [
            {"verdict_class": "machine_rejected", "patch_id": "p1", "reason": "same"},
            {"verdict_class": "machine_rejected", "patch_id": "p1", "reason": "same"},
        ]
        out = _dedupe_machine_entries(entries)
        self.assertEqual(len(out), 1)

    def test_keeps_machine_entries_with_different_reason(self):
        entries = [
            {"verdict_class": "machine_rejected", "patch_id": "p1", "reason": "a"},
            {"verdict_class": "machine_rejected", "patch_id": "p1", "reason": "b"},
        ]
        out = _dedupe_machine_entries(entries)
        self.assertEqual(len(out), 2)

    def test_human_entries_are_never_deduped_even_with_identical_identity(self):
        entries = [
            {"verdict_class": "human_rejected", "patch_id": "p1", "reason": "same"},
            {"verdict_class": "human_rejected", "patch_id": "p1", "reason": "same"},
        ]
        out = _dedupe_machine_entries(entries)
        self.assertEqual(len(out), 2)

    def test_machine_entries_missing_patch_id_or_reason_are_kept(self):
        entries = [
            {"verdict_class": "machine_rejected", "reason": "same"},
            {"verdict_class": "machine_rejected", "reason": "same"},
        ]
        out = _dedupe_machine_entries(entries)
        self.assertEqual(len(out), 2)


class SelectTierTests(unittest.TestCase):
    def test_empty_entries_returns_empty(self):
        self.assertEqual(_select_tier([], 4), [])

    def test_under_cap_keeps_everything_in_original_order(self):
        entries = [
            {"verdict_class": "machine_accepted", "tag": "a"},
            {"verdict_class": "human_rejected", "tag": "b"},
        ]
        self.assertEqual([e["tag"] for e in _select_tier(entries, 4)], ["a", "b"])

    def test_human_entries_preferred_over_machine_when_over_cap(self):
        # newest-first input; only 1 slot -- the (older) human entry must
        # still win over the (newer) machine entry.
        entries = [
            {"verdict_class": "machine_accepted", "tag": "newer_machine"},
            {"verdict_class": "human_rejected", "tag": "older_human"},
        ]
        self.assertEqual(_select_tier(entries, 1)[0]["tag"], "older_human")

    def test_remaining_slots_filled_by_machine_when_not_enough_humans(self):
        entries = [
            {"verdict_class": "machine_accepted", "tag": "m1"},
            {"verdict_class": "machine_accepted", "tag": "m2"},
            {"verdict_class": "human_rejected", "tag": "h1"},
        ]
        selected = _select_tier(entries, 2)
        self.assertEqual(len(selected), 2)
        self.assertIn("h1", [e["tag"] for e in selected])

    def test_selection_restored_to_newest_first_order(self):
        # Two humans (newer, older) and cap=2: both kept, but must come
        # back out newest-first, not human-then-machine-grouped.
        entries = [
            {"verdict_class": "human_rejected", "tag": "newer_human"},
            {"verdict_class": "machine_accepted", "tag": "middle_machine"},
            {"verdict_class": "human_rejected", "tag": "older_human"},
        ]
        selected = _select_tier(entries, 2)
        self.assertEqual([e["tag"] for e in selected], ["newer_human", "older_human"])


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


class LoadGlobalPitfallsTests(unittest.TestCase):
    """Spec K2: fresh read of <home>/logs/knowledge/GLOBAL-PITFALLS.md,
    falling back to the KNOWN_PITFALLS constant when missing/empty --
    tests stay hermetic by always pointing `home` at a tempdir."""

    def test_falls_back_to_known_pitfalls_when_missing(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(load_global_pitfalls(home=Path(tmpdir)), KNOWN_PITFALLS)

    def test_falls_back_to_known_pitfalls_when_file_is_blank(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            path = home / "logs" / "knowledge" / "GLOBAL-PITFALLS.md"
            path.parent.mkdir(parents=True)
            path.write_text("   \n")
            self.assertEqual(load_global_pitfalls(home=home), KNOWN_PITFALLS)

    def test_reads_seeded_content_instead_of_the_constant(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            path = home / "logs" / "knowledge" / "GLOBAL-PITFALLS.md"
            path.parent.mkdir(parents=True)
            path.write_text("- [seed] never paraphrase a PrintConv string\n")
            text = load_global_pitfalls(home=home)
        self.assertIn("never paraphrase a PrintConv string", text)
        self.assertNotEqual(text, KNOWN_PITFALLS)


class LoadModulePlaybookTests(unittest.TestCase):
    """Spec K3: <knowledge_home>/logs/knowledge/modules/<module>.md,
    written only by scripts/distill_lessons.py; workers only read."""

    def test_empty_when_no_knowledge_home(self):
        self.assertEqual(load_module_playbook(None, "Canon"), "")

    def test_empty_when_no_module_key(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(load_module_playbook(Path(tmpdir), None), "")

    def test_empty_when_file_missing(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(load_module_playbook(Path(tmpdir), "Canon"), "")

    def test_reads_the_module_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            modules_dir = home / "logs" / "knowledge" / "modules"
            modules_dir.mkdir(parents=True)
            (modules_dir / "Canon.md").write_text("- wrong_value x3: match Perl byte-for-byte")
            text = load_module_playbook(home, "Canon")
        self.assertIn("match Perl byte-for-byte", text)


class BuildPromptKnowledgeLayerTests(unittest.TestCase):
    """Spec K2/K3 wired into build_prompt: knowledge_home is None-gated
    exactly like every other optional source (perl_lib_dir,
    sweep_review_log_path) -- hermetic by default, never touching real
    ~/.oxidex unless a caller explicitly opts in."""

    def test_default_still_embeds_the_known_pitfalls_constant(self):
        # No knowledge_home given: build_prompt must never read the real
        # OXIDEX_HOME on a plain call -- this IS the hermeticity gate.
        prompt = build_prompt(make_gap(gap_count=1))
        self.assertIn(KNOWN_PITFALLS, prompt)

    def test_knowledge_home_swaps_in_the_curated_pitfalls_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            path = home / "logs" / "knowledge" / "GLOBAL-PITFALLS.md"
            path.parent.mkdir(parents=True)
            path.write_text("- [seed] curated pitfall marker XYZ\n")
            prompt = build_prompt(make_gap(gap_count=1), knowledge_home=home)
        self.assertIn("curated pitfall marker XYZ", prompt)
        self.assertNotIn(KNOWN_PITFALLS, prompt)

    def test_module_playbook_omitted_without_knowledge_home(self):
        prompt = build_prompt(make_gap(gap_count=1), module_name="Nikon")
        self.assertNotIn("Module playbook", prompt)

    def test_module_playbook_included_when_present(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            modules_dir = home / "logs" / "knowledge" / "modules"
            modules_dir.mkdir(parents=True)
            (modules_dir / "Nikon.md").write_text("- wrong_value x3: PrintConv byte match")
            prompt = build_prompt(make_gap(gap_count=1), knowledge_home=home, module_name="Nikon")
        self.assertIn("Module playbook", prompt)
        self.assertIn("PrintConv byte match", prompt)

    def test_module_name_falls_back_to_the_format_name(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            home = Path(tmpdir)
            modules_dir = home / "logs" / "knowledge" / "modules"
            modules_dir.mkdir(parents=True)
            # make_gap()'s format is "NEF" -- no module_name given, so the
            # format-name fallback key must be used (K3).
            (modules_dir / "NEF.md").write_text("- structural x2: fallback-key playbook")
            prompt = build_prompt(make_gap(gap_count=1), knowledge_home=home)
        self.assertIn("fallback-key playbook", prompt)


def _seed_lessons(home, rows):
    """Write K1 rows to <home>/logs/lessons.jsonl, ledger order."""
    path = Path(home) / "logs" / "lessons.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as f:
        for row in rows:
            f.write(json.dumps(row) + "\n")
    return path


def _lesson_row(event="build_failed", reason="r", worker="canon-1",
                module="Canon.pm", fmt="NEF", tag_key="", ts="2026-07-25T10:00:00"):
    return {"ts": ts, "worker": worker, "format": fmt, "module": module,
            "table": "", "tag_key": tag_key, "event": event, "reason": reason,
            "evidence": "", "checklist_id": ""}


def _seed_quarantine(home, rows):
    """Write squad_merge_loop-shaped entries to <home>/logs/quarantine.jsonl."""
    path = Path(home) / "logs" / "quarantine.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as f:
        for row in rows:
            f.write(json.dumps(row) + "\n")
    return path


def _quarantine_entry(squad="canon", fmt="NEF", flags=("printconv-unverifiable",),
                      reason="validate_fix_commit flags: printconv-unverifiable",
                      ts="2026-07-25T18:46:30"):
    return {"ts": ts, "patch_id": "p" + str(abs(hash((squad, fmt, reason))) % 10**8),
            "sha": "deadbeef", "format": fmt, "squad": squad, "reason": reason,
            "flags": list(flags), "attempt": 1, "backoff_seconds": 60}


class SquadFromWorkerTests(unittest.TestCase):
    """parallel_model_fix_loop mints worker ids as f"{squad}-{n}"; the
    quarantine ledger records only the squad. This is the link back --
    and it is validate_fix_commit's helper, reused rather than
    re-derived, because the two must agree on what "owns" means or the
    validator and the prompt disagree about whose commit was rejected."""

    def test_trailing_index_is_stripped(self):
        self.assertEqual(squad_from_worker("canon-3"), "canon")
        self.assertEqual(squad_from_worker("sony-minolta-2"), "sony-minolta")
        self.assertEqual(squad_from_worker("pentax-samsung-11"), "pentax-samsung")

    def test_a_label_with_no_index_suffix_is_its_own_squad(self):
        # run_worker's bare format name and main()'s default "1" never
        # encoded a squad. They must resolve to a squad name that matches
        # NOTHING in the ledger rather than to a wildcard -- see
        # OwnQuarantineTests.test_a_format_labelled_worker_is_never_shown
        # _another_squads_rejections for why that distinction is the
        # whole ballgame on the legacy (per-format) path.
        for label in ("JPEG", "1", "canon-"):
            self.assertEqual(squad_from_worker(label), label)


class OwnQuarantineTests(unittest.TestCase):
    """A worker never learned its own commit had been rejected, so it
    kept stacking new commits on the rejected code -- the root cause of
    an entire quarantine class (7 heads hitting cherry-pick conflicts
    purely because their own earlier commit had been dropped)."""

    def test_own_squad_and_format_entries_are_returned_newest_first(self):
        with tempfile.TemporaryDirectory() as home:
            path = _seed_quarantine(home, [
                _quarantine_entry(reason="older rejection", ts="2026-07-25T01:00:00"),
                _quarantine_entry(reason="newer rejection", ts="2026-07-25T02:00:00"),
            ])
            entries = read_own_quarantine(path, "canon-3", "NEF")
        self.assertEqual([e["reason"] for e in entries],
                         ["newer rejection", "older rejection"])

    def test_other_squads_and_other_formats_are_excluded(self):
        with tempfile.TemporaryDirectory() as home:
            path = _seed_quarantine(home, [
                _quarantine_entry(squad="canon", fmt="NEF", reason="mine"),
                _quarantine_entry(squad="nikon", fmt="NEF", reason="other squad"),
                _quarantine_entry(squad="canon", fmt="CR2", reason="other format"),
            ])
            entries = read_own_quarantine(path, "canon-3", "NEF")
        self.assertEqual([e["reason"] for e in entries], ["mine"])

    def test_squad_wide_bisection_verdict_has_no_format_and_still_matches(self):
        # overlord_sweep.bisect_sweep_failure quarantines a whole squad
        # with format_name=None: it rolled back the branch every member
        # shares, so every member needs to see it.
        with tempfile.TemporaryDirectory() as home:
            path = _seed_quarantine(home, [
                _quarantine_entry(squad="canon", fmt=None, flags=("sweep-bisection",),
                                  reason="overlord sweep bisection: isolated as the offending squad"),
            ])
            entries = read_own_quarantine(path, "canon-3", "NEF")
        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0]["flags"], ["sweep-bisection"])

    def test_a_format_labelled_worker_is_never_shown_another_squads_rejections(self):
        """The legacy per-format path -- parallel_model_fix_loop.run_round,
        which is the DEFAULT (run_squad_round needs --squad-mode) -- labels
        its workers with the bare format name (run_worker: worker_label =
        fmt) and model_fix_loop's own CLI defaults the label to "1".

        Neither encodes a squad, so an ownership test that degraded to
        "same format" for them made the (squad, format) pair collapse to
        FORMAT ALONE. Measured against the live ledger 2026-07-25 that is
        not an edge case, it is 100% wrong output: label "JPEG" returned
        four entries owned by ps-docs/sony-minolta/thermal, label "CR2"
        returned exif-core/canon, and the bare CLI label "1" returned
        ps-docs/sony-minolta/thermal -- every one of them rendered under
        "YOUR OWN commits that were REJECTED ... Fix the defect named by
        the flags below". A legacy worker has no quarantine entries of its
        own at all (the ledger records squads, and it is not in one), so
        the only correct answer for it is nothing.
        """
        with tempfile.TemporaryDirectory() as home:
            path = _seed_quarantine(home, [
                _quarantine_entry(squad="ps-docs", fmt="JPEG", reason="ps-docs rejection"),
                _quarantine_entry(squad="thermal", fmt="JPEG", reason="thermal rejection"),
                _quarantine_entry(squad="canon", fmt=None, flags=("sweep-bisection",),
                                  reason="squad-wide rollback of canon"),
            ])
            self.assertEqual(read_own_quarantine(path, "JPEG", "JPEG"), [])
            self.assertEqual(read_own_quarantine(path, "1", "JPEG"), [])
            self.assertEqual(read_own_quarantine(path, "CR2", "CR2"), [])

    def test_a_missing_format_trailer_verdict_is_squad_wide_not_invisible(self):
        """squad_merge_loop records the literal string "UNKNOWN" when a
        commit has no Format: trailer (squad_merge_loop.py:1111). "UNKNOWN"
        is truthy, so a plain `entry_format != format_name` test hid it
        from every worker: 28 of 77 live rows (36%, exactly 2 per squad
        across all 14 squads, 2026-07-25) were permanently unreachable --
        and ALL 28 carry `missing-trailer:Format` as their first flag.

        That is the one rejection class this whole feature exists to
        close, and it was the one class it could never report, so the
        worker kept omitting the trailer forever. A verdict with no usable
        format is squad-wide, exactly like overlord_sweep's formatless
        bisection entry.
        """
        with tempfile.TemporaryDirectory() as home:
            path = _seed_quarantine(home, [
                _quarantine_entry(squad="canon", fmt="UNKNOWN",
                                  flags=("missing-trailer:Format",),
                                  reason="validate_fix_commit flags: missing-trailer:Format"),
            ])
            entries = read_own_quarantine(path, "canon-3", "NEF")
        self.assertEqual([e["flags"] for e in entries], [["missing-trailer:Format"]])

    def test_bounded_by_max_entries_and_tolerant_of_garbage(self):
        with tempfile.TemporaryDirectory() as home:
            path = _seed_quarantine(home, [
                _quarantine_entry(reason=f"rejection {c}") for c in "abcdefghij"
            ])
            with path.open("a") as f:
                f.write("not json at all\n[1,2,3]\n")
            entries = read_own_quarantine(path, "canon-3", "NEF")
        self.assertEqual(len(entries), DEFAULT_QUARANTINE_MAX_ENTRIES)
        self.assertEqual(entries[0]["reason"], "rejection j")

    def test_a_malformed_flags_field_never_kills_the_worker(self):
        """`flags` is written by another process (squad_merge_loop) and is
        the one field in the read path that was consumed without coercion:
        `", ".join(str(f) for f in entry["flags"])` on a non-iterable
        raised TypeError straight out of build_prompt, through fix_gap,
        killing the worker process. Advisory context is never allowed to
        be a hard dependency (read_own_quarantine's own contract)."""
        for junk in (5, {"a": 1}, True, 3.5):
            entry = _quarantine_entry()
            entry["flags"] = junk
            rendered = format_own_quarantine([entry])
            self.assertIn("REJECTED", rendered)
        entry = _quarantine_entry()
        entry["flags"] = "printconv-mismatch:Auto"  # a bare string, not a list
        self.assertIn("printconv-mismatch:Auto", format_own_quarantine([entry]))

    def test_missing_ledger_is_not_an_error(self):
        with tempfile.TemporaryDirectory() as home:
            self.assertEqual(
                read_own_quarantine(Path(home) / "logs" / "quarantine.jsonl", "canon-3", "NEF"),
                [],
            )

    def test_rendered_section_leads_with_flags_and_clamps_the_reason(self):
        long_conflict = "cherry-pick failed: " + "CONFLICT line\n" * 200
        rendered = format_own_quarantine([
            _quarantine_entry(flags=("printconv-mismatch:Auto (bracketed)",),
                              reason=long_conflict),
        ])
        self.assertIn("printconv-mismatch:Auto (bracketed)", rendered)
        self.assertIn("REJECTED", rendered)
        # One bullet, one line: the multi-line git transcript is flattened
        # and clamped, never allowed to become 200 unindented lines.
        self.assertEqual(len(rendered.strip().splitlines()[-1:]), 1)
        self.assertLess(len(rendered.strip().splitlines()[-1]),
                        QUARANTINE_REASON_DISPLAY_CHARS + 120)

    def test_the_hidden_count_covers_flags_dropped_by_the_CHAR_cap_too(self):
        # Deriving `hidden` from the flag-COUNT cap alone made the suffix
        # lie: three 100-char flags against a 200-char budget rendered
        # 200 chars + "..." with hidden == 0, so the third flag vanished
        # silently. Both caps must feed the same count.
        text = model_fix_loop._clamp_quarantine_flags(["A" * 100, "B" * 100, "C" * 100])
        self.assertIn("(+2 more)", text)
        self.assertNotIn("C" * 100, text)

    def test_a_single_flag_longer_than_the_whole_budget_still_renders(self):
        text = model_fix_loop._clamp_quarantine_flags(["Z" * 500])
        self.assertTrue(text.startswith("Z"))
        self.assertTrue(text.endswith("..."))
        self.assertNotIn("more)", text, "nothing is hidden when there is one flag")

    def test_the_flags_list_is_capped_so_one_entry_cannot_eat_the_block(self):
        """validate_fix_commit appends one `printconv-mismatch:<48-char
        excerpt>` flag per unverifiable map value, uncapped -- the live
        worst case is already 11 flags / 512 chars (2026-07-25), and a
        commit adding a ~30-entry PrintConv lookup produces 30 flags in
        one entry. format_own_quarantine joined them with no cap while its
        docstring promised "Deterministic, bounded output"."""
        entries = [
            _quarantine_entry(
                reason=f"validate_fix_commit flags: printconv-mismatch ({n})",
                flags=tuple(f"printconv-mismatch:Auto (bracketed, +{i} EV) sample {n}"
                            for i in range(30)),
            )
            for n in range(DEFAULT_QUARANTINE_MAX_ENTRIES)
        ]
        rendered = format_own_quarantine(entries)
        # The docstring's promise, made testable: header + at most
        # DEFAULT_QUARANTINE_MAX_ENTRIES lines, each bounded by the flags
        # cap plus the reason cap plus rendering punctuation.
        for line in rendered.strip().splitlines()[1:]:
            self.assertLessEqual(
                len(line),
                QUARANTINE_FLAGS_DISPLAY_CHARS + QUARANTINE_REASON_DISPLAY_CHARS + 80,
                line,
            )
        self.assertIn("printconv-mismatch:", rendered)

    def test_a_flag_heavy_entry_cannot_starve_the_rest_of_the_learning_block(self):
        """LEARNING_SECTION_ORDER ranks the quarantine section SECOND, so
        anything it fails to bound is taken straight out of K4 sweep
        reviews, the K3 module playbook and the lessons tail. Reproduced
        before the cap: four 30-flag entries rendered a 5858-char block
        against a 4800-char budget, and compose_learning_block admitted
        the quarantine block ALONE -- the K3 knowledge spine went dark for
        that worker for as long as the entry stayed in the 64KB window,
        which today is the whole 48KB ledger, i.e. indefinitely."""
        gap = make_gap(gap_count=1)
        with tempfile.TemporaryDirectory() as home:
            _seed_quarantine(home, [
                _quarantine_entry(
                    squad="canon", fmt="NEF",
                    reason=f"validate_fix_commit flags: printconv-mismatch ({n})",
                    flags=tuple(f"printconv-mismatch:Auto (bracketed, +{i} EV) s{n}"
                                for i in range(30)),
                )
                for n in range(DEFAULT_QUARANTINE_MAX_ENTRIES)
            ])
            _seed_lessons(home, [
                _lesson_row(event="wrong_value", reason="THE RECURRING MISTAKE")
            ] * 4)
            modules_dir = Path(home) / "logs" / "knowledge" / "modules"
            modules_dir.mkdir(parents=True)
            (modules_dir / "Canon.pm.md").write_text("- THE MODULE PLAYBOOK BULLET\n")
            prompt = build_prompt(gap, knowledge_home=Path(home),
                                  module_name="Canon.pm", worker_label="canon-3")
        self.assertIn("REJECTED", prompt)
        self.assertIn("THE MODULE PLAYBOOK BULLET", prompt)
        self.assertIn("THE RECURRING MISTAKE", prompt)


class BuildPromptOwnQuarantineTests(unittest.TestCase):
    def test_a_workers_own_rejection_flags_reach_its_prompt(self):
        with tempfile.TemporaryDirectory() as home:
            _seed_quarantine(home, [
                _quarantine_entry(squad="canon", fmt="NEF",
                                  flags=("printconv-mismatch:Auto (bracketed)",),
                                  reason="validate_fix_commit flags: printconv-mismatch"),
            ])
            prompt = build_prompt(make_gap(gap_count=1), knowledge_home=Path(home),
                                  worker_label="canon-3")
        self.assertIn("printconv-mismatch:Auto (bracketed)", prompt)
        self.assertIn("REJECTED", prompt)

    def test_another_workers_rejection_does_not(self):
        with tempfile.TemporaryDirectory() as home:
            _seed_quarantine(home, [
                _quarantine_entry(squad="nikon", fmt="NEF",
                                  flags=("targeted-test-failed",),
                                  reason="cargo test --lib nef failed"),
            ])
            prompt = build_prompt(make_gap(gap_count=1), knowledge_home=Path(home),
                                  worker_label="canon-3")
        self.assertNotIn("targeted-test-failed", prompt)
        self.assertNotIn("REJECTED", prompt)

    def test_omitted_entirely_without_a_worker_label(self):
        # Hermetic-by-default: every pre-existing caller passes no
        # worker_label and must keep its byte-identical prompt.
        with tempfile.TemporaryDirectory() as home:
            _seed_quarantine(home, [_quarantine_entry(squad="canon", fmt="NEF")])
            with_label = build_prompt(make_gap(gap_count=1), knowledge_home=Path(home),
                                      worker_label="canon-3")
            without = build_prompt(make_gap(gap_count=1), knowledge_home=Path(home))
        self.assertIn("printconv-unverifiable", with_label)
        self.assertNotIn("printconv-unverifiable", without)


class InfraNoiseNeverReachesAPromptTests(unittest.TestCase):
    """226 urlopen errors and 54 HTTP 429s are provider outages, not
    knowledge. They must not reach a prompt and must not consume a byte
    of the learning budget -- including the 231 live rows that labelled
    an outage as build_failed/review_rejected/structural."""

    INFRA_ROWS = (
        ("infra", "model call failed: HTTP Error 429: Too Many Requests"),
        ("build_failed", "model call failed: <urlopen error [Errno 8] nodename "
                         "nor servname provided, or not known>"),
        ("review_rejected", "review call failed: [Errno 54] Connection reset by peer"),
        ("structural", "FAILED NEF:EXIF:CFAPattern2: model call failed: "
                       "[Errno 54] Connection reset by peer"),
    )

    def test_tail_reader_drops_them_however_they_were_labelled(self):
        with tempfile.TemporaryDirectory() as home:
            path = _seed_lessons(home, [
                _lesson_row(event=event, reason=reason)
                for event, reason in self.INFRA_ROWS
            ] + [_lesson_row(event="wrong_value", reason="a genuine lesson")])
            events = read_lessons_tail_events(path)
        self.assertEqual([e["reason"] for e in events], ["a genuine lesson"])

    def test_none_of_it_reaches_the_prompt(self):
        with tempfile.TemporaryDirectory() as home:
            _seed_lessons(home, [
                _lesson_row(event=event, reason=reason)
                for event, reason in self.INFRA_ROWS
            ])
            prompt = build_prompt(make_gap(gap_count=1), knowledge_home=Path(home),
                                  module_name="Canon.pm", worker_label="canon-1")
        for token in ("urlopen", "429", "Connection reset", "model call failed",
                      "Recent lessons ledger entries"):
            self.assertNotIn(token, prompt)

    def test_it_consumes_no_learning_budget(self):
        """Hundreds of outage rows must produce a byte-identical prompt
        to zero of them. The block admits max_entries DISTINCT lessons;
        if an outage counted as one, a single provider incident would
        evict every real lesson from the block for hours."""
        gap = make_gap(gap_count=1)
        # Outages first, real lessons last: if the outages consumed any
        # ranking slot or any byte of budget, the real lessons appended
        # after them would be the ones pushed out.
        outage = [_lesson_row(event=event, reason=reason)
                  for event, reason in self.INFRA_ROWS] * 50
        real = [_lesson_row(event="wrong_value", reason=f"real lesson {c}")
                for c in "abcdefgh"]
        with tempfile.TemporaryDirectory() as quiet:
            _seed_lessons(quiet, real)
            quiet_prompt = build_prompt(gap, knowledge_home=Path(quiet),
                                        module_name="Canon.pm", worker_label="canon-1")
        with tempfile.TemporaryDirectory() as noisy:
            _seed_lessons(noisy, outage + real)
            noisy_prompt = build_prompt(gap, knowledge_home=Path(noisy),
                                        module_name="Canon.pm", worker_label="canon-1")
        self.assertIn("real lesson a", quiet_prompt)
        self.assertEqual(noisy_prompt, quiet_prompt)


class AdaptiveDiffFormatGuidanceTests(unittest.TestCase):
    """"no diff in model response" is 1489 of 4922 live lesson rows
    (~40% of all failures) and is a RESPONSE-FORMAT failure, not a
    domain-knowledge one. The corrective text goes only to the workers
    actually failing that way."""

    NO_DIFF_SPELLINGS = (
        "no diff in model response",
        "no diff in model response (exhausted request budget)",
        "no diff in model response (exhausted verify budget)",
        "no diff in model response (patch chunking exceeded safety limit)",
    )

    def test_counts_only_this_workers_rows(self):
        events = [
            _lesson_row(worker="canon-1", reason=self.NO_DIFF_SPELLINGS[0]),
            _lesson_row(worker="canon-1", reason=self.NO_DIFF_SPELLINGS[2]),
            _lesson_row(worker="nikon-1", reason=self.NO_DIFF_SPELLINGS[0]),
            _lesson_row(worker="canon-1", reason="gap count did not decrease"),
        ]
        self.assertEqual(count_diff_format_failures(events, "canon-1"), 2)
        self.assertEqual(count_diff_format_failures(events, "nikon-1"), 1)
        self.assertEqual(count_diff_format_failures(events, "olympus-1"), 0)
        self.assertEqual(count_diff_format_failures(events, None), 0)

    def test_every_attempt_build_spelling_is_recognized(self):
        for reason in self.NO_DIFF_SPELLINGS:
            self.assertEqual(
                count_diff_format_failures([_lesson_row(reason=reason)], "canon-1"), 1,
                reason,
            )

    def test_guidance_states_the_envelope_with_a_working_example(self):
        text = build_diff_format_remediation(3)
        self.assertIn("```diff", text)
        self.assertIn("--- a/", text)   # a bare @@ hunk does not apply
        self.assertIn("+++ b/", text)
        self.assertIn("EXACTLY ONE", text)
        self.assertIn("3 recent", text)
        self.assertEqual(build_diff_format_remediation(0), "")

    def test_it_never_spends_a_slot_in_the_domain_lessons_ranking(self):
        """Each failure class is routed to the one section that can fix
        it. Without this, ranking by recurrence is self-defeating: over
        the live tail 2026-07-25 this reason is 40 of 235 rows and ranks
        FIRST for essentially every module, evicting the wrong_value and
        structural lessons that carry the actual ExifTool knowledge."""
        # fix_gap writes TWO rows per failed round, not one: the specific
        # event, and then critique_and_continue's lesson("critique",
        # critique). The critique row carries the CRITIC'S PARAPHRASE, so
        # it never contains the literal "no diff in model response" and a
        # literal-match filter sails straight past it. The old fixture
        # wrote only the first row, which is the only reason this test
        # ever passed. Measured on the live tail 2026-07-25: 14 of 43
        # ranked slots across the 6 live formats were spent on that
        # paraphrase (MRW 5 of 6, NEF 4 of 6).
        events = []
        for i in range(40):
            events.append(_lesson_row(reason=self.NO_DIFF_SPELLINGS[i % 4]))
            events.append(_lesson_row(
                event="critique",
                reason=(f"The fixer likely emitted no diff because it assumed "
                        f"tag {i} was already handled, and provided a prose "
                        f"description instead of an actual unified diff."),
            ))
        events += [_lesson_row(event="wrong_value", reason="PrintConv byte match")] * 2
        ranked = select_module_lessons(events, "Canon.pm", "NEF")
        self.assertEqual([(e["reason"], n) for e, n in ranked],
                         [("PrintConv byte match", 2)])

    def test_failing_worker_gets_it_and_a_clean_worker_does_not(self):
        gap = make_gap(gap_count=1)
        with tempfile.TemporaryDirectory() as home:
            _seed_lessons(home, [
                _lesson_row(worker="canon-1", reason=self.NO_DIFF_SPELLINGS[0]),
            ])
            failing = build_prompt(gap, knowledge_home=Path(home),
                                   module_name="Canon.pm", worker_label="canon-1")
            clean = build_prompt(gap, knowledge_home=Path(home),
                                 module_name="Canon.pm", worker_label="canon-2")
        self.assertIn("FORMAT ALERT", failing)
        self.assertNotIn("FORMAT ALERT", clean)

    def test_the_alert_does_not_contradict_the_reply_shape_manifest(self):
        """The alert and build_reply_shape_manifest ship in the SAME
        prompt, and the alert used to overrule it: it said the reply "is
        parsed by looking for a ```diff fenced block, and nothing else
        counts" and "Prose describing the change ... discarded unread",
        while the manifest defines four legal shapes of which shape 1
        (REQUEST) is a bare prose line with NO diff at all and shape 4
        REQUIRES 2-3 sentences of prose before the fence -- and the
        STRATEGY paragraph tells the worker to probe with VERIFY.

        A worker that believes the alert stops issuing REQUEST/VERIFY,
        which is the opposite of what the alert is for.
        """
        text = build_diff_format_remediation(3)
        manifest = build_reply_shape_manifest(4096)
        for shape in ("REQUEST", "VERIFY", "PATCH"):
            self.assertIn(shape, manifest)
            self.assertIn(shape, text, f"the alert must not erase shape {shape}")
        self.assertNotIn("nothing else counts", text)
        self.assertNotIn("Nothing may follow the closing fence", text)

    def test_the_alert_makes_no_claim_extract_diff_contradicts(self):
        """Two of the alert's assertions were false in the STRICT
        direction, which is the direction that costs rounds: extract_diff
        also accepts an unfenced reply that starts with "diff --git"/
        "--- ", and DIFF_BLOCK_RE is a NON-GREEDY search, so trailing text
        after the closing fence is simply ignored. Pin both to
        extract_diff's actual behaviour so the text and the parser can
        never drift apart again."""
        body = ("--- a/src/parsers/jpeg/foo.rs\n"
                "+++ b/src/parsers/jpeg/foo.rs\n"
                "@@ -1,2 +1,3 @@\n a\n+b\n c\n")
        # First half: what the parser really accepts.
        self.assertIsNotNone(extract_diff(body))                       # unfenced
        self.assertIsNotNone(extract_diff(f"```diff\n{body}```\nthanks!"))  # trailing prose
        self.assertIsNotNone(extract_diff(f"Here is the fix.\n\n```diff\n{body}```"))

        # Second half, and WITHOUT IT THIS TEST GUARDS NOTHING: assert the
        # alert text makes no claim the three cases above contradict.
        # Reverting DIFF_FORMAT_REMEDIATION wholesale to its old
        # self-contradicting wording left this test green, because every
        # assertion was on extract_diff -- a function the change never
        # touched. The docstring's promise ("so the text and the parser can
        # never drift apart again") requires binding BOTH sides.
        alert = model_fix_loop.DIFF_FORMAT_REMEDIATION
        for false_in_the_strict_direction in (
            "nothing else counts",
            "Nothing may follow the closing fence",
        ):
            self.assertNotIn(false_in_the_strict_direction, alert)
        # And it must not discourage the evidence-gathering protocol the
        # loop documents as its own fastest path to convergence.
        for shape in ("REQUEST", "VERIFY"):
            self.assertIn(shape, alert)


class RecurrenceRankingOnProseRowsTests(unittest.TestCase):
    """rank_by_recurrence clusters on fingerprint_scoped, whose reason
    component is NORMALIZED FREE TEXT. That works for the machine-written
    reasons (cargo output, tag-key mismatches) and not at all for the
    critic's multi-hundred-word prose.

    Measured over the live 256KB tail 2026-07-25 (235 rows), per event:
    critique 112 rows -> 107 clusters, 106 of them singletons (95%);
    build_failed 78 rows -> 2 clusters, 0 singletons; gap_not_closed 11
    -> 1 cluster; review_rejected 10 -> 2 clusters. The degeneration is
    entirely and exclusively the critique event, and a ranking in which
    every row is a singleton is not a recurrence ranking -- ties fall
    through to recency, i.e. exactly the newest-first tail this feature
    was introduced to replace, for 43% of the ledger.
    """

    #: Verbatim shapes from the live ledger (worker ids and module names
    #: elided). Note they vary LEXICALLY, not numerically -- norm_reason
    #: folds digit runs to "#", so a fixture that varied only by an index
    #: would cluster and prove nothing.
    LIVE_CRITIQUE_SHAPES = (
        "The model most likely assumed the missing tag belonged in the top-level "
        "container parser rather than in the specific submodule that actually "
        "hosts that tag table, causing it to return an empty result.",
        "The fixer most likely failed because it searched for a tag definition "
        "table and found none, incorrectly concluding no diff was needed.",
        "You likely either explained the fix in prose or assumed that ISO tag "
        "coverage is already handled elsewhere, so you emitted no code patch.",
        "You likely assumed CR2 has a dedicated module or top-level tag table, "
        "causing the model to target a non-existent file.",
        "The attempt probably reused a PrintConv string remembered from another "
        "camera family instead of reading the one in the maker-note table.",
    )

    def test_prose_critiques_do_not_displace_genuinely_recurring_lessons(self):
        events = [_lesson_row(event="wrong_value", reason="THE RECURRING MISTAKE")] * 3
        events += [
            _lesson_row(event="critique", reason=shape)
            for _ in range(8) for shape in self.LIVE_CRITIQUE_SHAPES
        ]
        ranked = select_module_lessons(events, "Canon.pm", "NEF")
        self.assertEqual([(e["reason"], n) for e, n in ranked],
                         [("THE RECURRING MISTAKE", 3)])

    def test_machine_written_reasons_still_cluster_and_still_rank(self):
        # The fix must not throw out the ranking itself: reasons that DO
        # carry a repeatable identity keep clustering exactly as before.
        events = [_lesson_row(event="build_failed", reason="error[E0308]: mismatched types")] * 5
        events += [_lesson_row(event="wrong_value", reason="PrintConv byte match")] * 2
        events += [_lesson_row(event="structural", reason="double emission of Make")]
        ranked = select_module_lessons(events, "Canon.pm", "NEF")
        self.assertEqual([(e["reason"], n) for e, n in ranked],
                         [("error[E0308]: mismatched types", 5),
                          ("PrintConv byte match", 2),
                          ("double emission of Make", 1)])


class DeadCompatibilityShimTests(unittest.TestCase):
    def test_read_lessons_tail_is_gone(self):
        """It was kept as a compatibility shim for callers that do not
        exist: zero callers and zero tests in the tree, while its return
        type silently changed from [event, ...] to [(event, count), ...]
        when recurrence ranking landed. A shim nothing calls, nothing
        tests, and whose contract changed under it is not compatibility,
        it is a trap for the next caller."""
        self.assertFalse(hasattr(model_fix_loop, "read_lessons_tail"))


class LessonRecurrenceRankingTests(unittest.TestCase):
    """Section 6: the block leads with the mistake this module keeps
    repeating, not with whatever happened last."""

    def test_repeated_fingerprint_outranks_a_more_recent_one_off(self):
        events = [_lesson_row(event="wrong_value", reason="PrintConv must match Perl")] * 3
        events.append(_lesson_row(event="build_failed", reason="a one-off compile error"))
        ranked = select_module_lessons(events, "Canon.pm", "NEF")
        self.assertEqual([(e["reason"], n) for e, n in ranked], [
            ("PrintConv must match Perl", 3),
            ("a one-off compile error", 1),
        ])

    def test_module_scoping_beats_format_scoping_when_a_module_is_given(self):
        events = [
            _lesson_row(module="Canon.pm", fmt="NEF", reason="canon lesson"),
            _lesson_row(module="Nikon.pm", fmt="NEF", reason="nikon lesson"),
        ]
        self.assertEqual([e["reason"] for e, _ in select_module_lessons(events, "Canon.pm", "NEF")],
                         ["canon lesson"])
        # No module key -> format scoping, which here admits both rows.
        self.assertEqual(len(select_module_lessons(events, None, "NEF")), 2)

    def test_rendered_block_shows_the_recurrence_count_only_when_repeated(self):
        events = [_lesson_row(event="wrong_value", reason="recurring")] * 4
        events.append(_lesson_row(event="build_failed", reason="one-off"))
        rendered = format_lessons_tail(select_module_lessons(events, "Canon.pm", "NEF"))
        self.assertIn("- wrong_value x4: recurring", rendered)
        self.assertIn("- build_failed: one-off", rendered)
        self.assertLess(rendered.index("x4"), rendered.index("one-off"))

    def test_recurring_mistake_leads_the_prompt_block(self):
        with tempfile.TemporaryDirectory() as home:
            rows = [_lesson_row(event="wrong_value", reason="THE RECURRING MISTAKE")] * 5
            rows += [_lesson_row(event="build_failed", reason=f"a fresh one-off {c}")
                     for c in "abcdefgh"]
            _seed_lessons(home, rows)
            prompt = build_prompt(make_gap(gap_count=1), knowledge_home=Path(home),
                                  module_name="Canon.pm")
        self.assertIn("wrong_value x5: THE RECURRING MISTAKE", prompt)
        self.assertLess(prompt.index("THE RECURRING MISTAKE"), prompt.index("a fresh one-off"))


class LearningBlockBudgetTests(unittest.TestCase):
    """The budget must stay honest no matter how large the ledgers grow,
    and the squeeze must fall on the least valuable sections."""

    def test_composition_is_bounded_by_the_budget(self):
        parts = {name: "x" * 10_000 for name in LEARNING_SECTION_ORDER}
        self.assertEqual(len(compose_learning_block(parts, 100)), 400)
        self.assertEqual(compose_learning_block(parts, 0), "")

    def test_overflow_is_shed_from_the_tail_of_the_priority_order(self):
        parts = {"diff_format": "F" * 40, "quarantine": "Q" * 40,
                 "sweep_reviews": "S" * 40, "module_playbook": "M" * 40,
                 "lessons_tail": "L" * 40}
        text = compose_learning_block(parts, 25)  # 100 chars
        self.assertEqual(text, "F" * 40 + "Q" * 40 + "S" * 20)
        self.assertNotIn("M", text)
        self.assertNotIn("L", text)

    def test_unranked_keys_are_ignored(self):
        self.assertEqual(compose_learning_block({"mystery": "Z" * 50}, 100), "")

    def test_pre_existing_three_section_order_is_unchanged(self):
        # With neither new section present the block must be exactly what
        # the old `sweep + module + tail` concatenation produced, so a
        # worker with no quarantine record and no format problem keeps a
        # byte-identical (cacheable) prompt.
        parts = {"sweep_reviews": "S", "module_playbook": "M", "lessons_tail": "L"}
        self.assertEqual(compose_learning_block(parts, 1000), "SML")

    def test_large_synthetic_ledger_stays_within_the_configured_budget(self):
        gap = make_gap(gap_count=1)
        budget = DEFAULT_LEARNING_BUDGET_TOKENS  # 1200 tokens -> 4800 chars
        with tempfile.TemporaryDirectory() as home:
            # ~3000 lesson rows, 400 quarantine entries and a full-size
            # playbook: every section of the block over-full at once.
            rows = []
            for i in range(1000):
                rows.append(_lesson_row(event="wrong_value",
                                        reason=f"recurring mistake {chr(97 + i % 26)}" * 8))
                rows.append(_lesson_row(event="build_failed", reason="x" * 400))
                rows.append(_lesson_row(worker="canon-1",
                                        reason="no diff in model response"))
            _seed_lessons(home, rows)
            _seed_quarantine(home, [
                _quarantine_entry(flags=("printconv-mismatch:Auto (bracketed)",),
                                  reason="y" * 900)
                for _ in range(400)
            ])
            modules_dir = Path(home) / "logs" / "knowledge" / "modules"
            modules_dir.mkdir(parents=True)
            (modules_dir / "Canon.pm.md").write_text("- playbook bullet\n" * 300)

            # learning_budget_tokens=0 empties the block entirely, so the
            # difference between the two prompts IS the block, exactly --
            # no estimating around the rest of the prompt.
            without_block = build_prompt(gap, knowledge_home=Path(home),
                                         module_name="Canon.pm", worker_label="canon-1",
                                         learning_budget_tokens=0)
            loaded = build_prompt(gap, knowledge_home=Path(home),
                                  module_name="Canon.pm", worker_label="canon-1",
                                  learning_budget_tokens=budget)
        self.assertLessEqual(len(loaded) - len(without_block), budget * 4)
        # The highest-priority sections are the ones that survived...
        self.assertIn("FORMAT ALERT", loaded)
        self.assertIn("printconv-mismatch:Auto (bracketed)", loaded)
        # ...and the lowest-priority one was shed entirely.
        self.assertNotIn("Recent lessons ledger entries", loaded)


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

    def test_unreachable_reviewer_does_not_kill_the_worker(self):
        # Continue-on policy: an unreachable reviewer is "not approved
        # this round", not a crash. The tag comes back around and gets
        # reviewed again once the provider answers.
        gap = make_gap()

        def raising(messages, *a):
            raise ModelQuotaExhausted("weekly cost limit reached")

        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k",
             "models": [{"name": "gpt-5.6-terra", "base_url": "u", "api_key": "k"}],
             "max_tokens": 4096, "reasoning_effort": "low"},
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


class ParseChecklistIdTests(unittest.TestCase):
    def test_finds_leading_checklist_token(self):
        from model_fix_loop import parse_checklist_id
        self.assertEqual(parse_checklist_id("C2 PrintConv paraphrased"), "C2")

    def test_finds_checklist_token_anywhere_in_text(self):
        from model_fix_loop import parse_checklist_id
        self.assertEqual(parse_checklist_id("hardcodes value, see C5 above"), "C5")

    def test_none_when_absent(self):
        from model_fix_loop import parse_checklist_id
        self.assertIsNone(parse_checklist_id("no checklist id here"))
        self.assertIsNone(parse_checklist_id(None))


class ExtractReviewVerdictFullTests(unittest.TestCase):
    def test_approve(self):
        from model_fix_loop import extract_review_verdict_full
        self.assertEqual(extract_review_verdict_full("APPROVE"), ("approve", ""))

    def test_reject_with_checklist_id(self):
        from model_fix_loop import extract_review_verdict_full
        verdict, reason = extract_review_verdict_full("REJECT: C2 paraphrased PrintConv")
        self.assertEqual(verdict, "reject")
        self.assertEqual(reason, "C2 paraphrased PrintConv")

    def test_unverifiable_with_checklist_id(self):
        from model_fix_loop import extract_review_verdict_full
        verdict, reason = extract_review_verdict_full(
            "UNVERIFIABLE: C1 the Perl table wasn't shown in the prompt")
        self.assertEqual(verdict, "unverifiable")
        self.assertEqual(reason, "C1 the Perl table wasn't shown in the prompt")

    def test_unparseable_defaults_to_reject(self):
        from model_fix_loop import extract_review_verdict_full
        verdict, reason = extract_review_verdict_full("uh, maybe?")
        self.assertEqual(verdict, "reject")
        self.assertIn("unparseable review verdict", reason)

    def test_trailing_approve_after_checklist_is_honored(self):
        # The review prompt says "answer each checklist item briefly, THEN
        # give your verdict" -- a model that OBEYS puts APPROVE last. The
        # first-line-only match scored these unparseable -> REJECT,
        # measured at 7/209 real reviews and destroying already-built,
        # already-gap-verified fixes.
        from model_fix_loop import extract_review_verdict_full
        reply = (
            "C1: exact tag ID match confirmed (0x0112).\n"
            "C2: PrintConv strings are byte-identical to Exif.pm.\n"
            "C3: single reachable emitter.\n"
            "C4: test asserts against a real sample.\n"
            "C5: no hardcoded values.\n"
            "\n"
            "APPROVE"
        )
        self.assertEqual(extract_review_verdict_full(reply), ("approve", ""))

    def test_trailing_verdict_label_and_markdown_emphasis_are_tolerated(self):
        from model_fix_loop import extract_review_verdict_full
        self.assertEqual(
            extract_review_verdict_full("C1: fine.\n\n**Final Verdict:** APPROVE"),
            ("approve", ""),
        )
        verdict, reason = extract_review_verdict_full(
            "C2: paraphrased.\n\nVerdict: REJECT: C2 paraphrased PrintConv")
        self.assertEqual(verdict, "reject")
        self.assertEqual(reason, "C2 paraphrased PrintConv")

    def test_last_verdict_line_wins_over_criteria_discussion(self):
        # Checklist bodies routinely mention the words approve/reject while
        # discussing criteria; the model's real conclusion is the LAST one,
        # which is why the rescan runs bottom-up rather than top-down.
        from model_fix_loop import extract_review_verdict_full
        reply = (
            "I would normally approve a change like this, but:\n"
            "C2: the PrintConv string was paraphrased.\n"
            "\n"
            "REJECT: C2 paraphrased PrintConv"
        )
        verdict, reason = extract_review_verdict_full(reply)
        self.assertEqual(verdict, "reject")
        self.assertEqual(reason, "C2 paraphrased PrintConv")

    def test_no_verdict_anywhere_still_fails_safe_to_reject(self):
        from model_fix_loop import extract_review_verdict_full
        verdict, reason = extract_review_verdict_full(
            "C1: looks plausible.\nC2: I am not sure about the table.\nHmm.")
        self.assertEqual(verdict, "reject")
        self.assertIn("unparseable review verdict", reason)

    def test_extract_review_verdict_delegates_and_folds_unverifiable_to_not_approved(self):
        # The preserved two-tuple contract can't represent a third state,
        # so UNVERIFIABLE degrades to "not approved" here -- callers that
        # need the nuance use review_verdict/extract_review_verdict_full.
        approved, reason = extract_review_verdict("UNVERIFIABLE: C1 missing evidence")
        self.assertFalse(approved)


class ReviewVerdictUnverifiableTests(unittest.TestCase):
    def test_unverifiable_reply_is_approved_with_prefixed_reason(self):
        gap = make_gap(gap_count=2)
        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "models": [{"name": "m", "base_url": "u", "api_key": "k"}],
             "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=lambda messages, *a: "UNVERIFIABLE: C1 Perl table not shown",
        )
        self.assertTrue(approved)
        self.assertTrue(reason.startswith("UNVERIFIABLE:"))
        self.assertIn("C1", reason)

    def test_evidence_kwargs_reach_build_review_prompt(self):
        gap = make_gap(gap_count=2)
        seen_prompts = []

        def tracking_call_model_fn(messages, *a):
            seen_prompts.append(messages[0]["content"])
            return "APPROVE"

        review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "models": [{"name": "m", "base_url": "u", "api_key": "k"}],
             "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=tracking_call_model_fn,
            perl_block="\n\nPERL-MARKER", live_evidence="EVIDENCE-MARKER",
            emission_scan="SCAN-MARKER",
        )
        self.assertIn("PERL-MARKER", seen_prompts[0])
        self.assertIn("EVIDENCE-MARKER", seen_prompts[0])
        self.assertIn("SCAN-MARKER", seen_prompts[0])


class BuildReviewPromptChecklistTests(unittest.TestCase):
    def test_includes_c1_through_c5_and_unverifiable_reply_shape(self):
        gap = make_gap(gap_count=2)
        prompt = build_review_prompt(gap, "--- a/x\n+++ b/x\n")
        for needle in ("C1", "C2", "C3", "C4", "C5", "UNVERIFIABLE:"):
            self.assertIn(needle, prompt)

    def test_evidence_sections_included_when_given(self):
        gap = make_gap(gap_count=2)
        prompt = build_review_prompt(
            gap, "--- a/x\n+++ b/x\n",
            perl_block="\n\nPERL-BLOCK-MARKER",
            live_evidence="exiftool=1 oxidex=2",
            emission_scan="src/parsers/x.rs:10:foo",
        )
        self.assertIn("PERL-BLOCK-MARKER", prompt)
        self.assertIn("exiftool=1 oxidex=2", prompt)
        self.assertIn("src/parsers/x.rs:10:foo", prompt)


class HermeticFixGapTestCase(unittest.TestCase):
    """Shared base for every TestCase below that calls fix_gap.

    fix_gap defaults extract_evidence_fn/scan_fn to
    default_extract_live_evidence/default_emission_scan (spec K5's real
    subprocess-backed implementations -- they shell out to exiftool, rg,
    and `git show HEAD:<path>` against repo_root) whenever a caller
    doesn't pass its own. Most fix_gap tests below exercise the
    post-build review path without ever wiring a fake for either
    parameter, which means they'd otherwise silently inherit those real,
    non-hermetic defaults against whatever repo_root resolves to --
    violating the house rule (hermetic Python tests: no network/cargo/
    real ~/.oxidex; injectable fns). Patching the two names on
    model_fix_loop's own module object (not this test module's imported
    copy) is what actually intercepts them: fix_gap's `extract_evidence_fn
    or default_extract_live_evidence` is a bare-name lookup resolved via
    fix_gap's own __globals__ (model_fix_loop's module dict) at call
    time, not a reference captured at def-time. A test that passes its
    own extract_evidence_fn/scan_fn is unaffected -- fix_gap only falls
    back to the (now-patched) module default when the caller's argument
    is falsy."""

    def setUp(self):
        super().setUp()
        evidence_patcher = patch(
            "model_fix_loop.default_extract_live_evidence",
            lambda repo_root, sample_path, tag_keys: "",
        )
        scan_patcher = patch(
            "model_fix_loop.default_emission_scan",
            lambda repo_root, parser_files, tag_keys, diff_text=None: "",
        )
        evidence_patcher.start()
        scan_patcher.start()
        self.addCleanup(evidence_patcher.stop)
        self.addCleanup(scan_patcher.stop)


class FixGapHappyPathTests(HermeticFixGapTestCase):
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
            git_commit_fn=lambda msg, root, **_kw: commit_calls.append(msg),
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

    def test_exhausted_quota_is_an_infra_failure_the_worker_continues_past(self):
        # Operator policy: empty 200s, 429s and everything in that class
        # are continued past, never fatal. The reason must carry
        # INFRA_FAILURE_PREFIX so run_tag_loop's infra_only branch charges
        # nothing -- no fail increment, no blacklist. Propagating instead
        # would kill the worker, which is the opposite of continuing on.
        def raising_call_model(messages, *a):
            raise ModelQuotaExhausted("weekly cost limit reached")

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
        self.assertTrue(reason.startswith(INFRA_FAILURE_PREFIX), reason)
        self.assertIn("weekly cost limit", reason)

    def test_empty_reply_exhaustion_is_also_an_infra_failure(self):
        # The dominant real-world failure: HTTP 200 with an empty body
        # (29.1% of gpt-5.5 calls, 5.9% of gpt-5.6-terra). Must land in
        # the same continue-on bucket as a 429.
        def raising_call_model(messages, *a):
            raise RuntimeError("model returned an empty reply")

        built, reason, _diff, _msgs = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=raising_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertTrue(reason.startswith(INFRA_FAILURE_PREFIX), reason)

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
            # 6th call: this is the forced-diff turn -- submit a real diff.
            self.assertIn("reply with a diff and nothing else", messages[-1]["content"])
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

    # --- investigation-budget visibility (defect 3) -------------------------
    #
    # RW2 transcript, 2026-07-26T21:23: the model spent turn 24 on yet another
    # REQUEST because nothing in the conversation ever told it how many
    # investigation turns it had or how many were left, and the one
    # "stop investigating" message only arrived AFTER the budget was gone.

    def test_every_request_answer_ends_with_the_remaining_budget(self):
        seen = []

        def fake_call_model(messages, *a):
            if messages[-1]["role"] == "user" and len(seen) < 3:
                seen.append(messages[-1]["content"])
            if len(seen) < 3:
                return "REQUEST: src/x.rs"
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "x.rs").write_text("fn a() {}\n")
            attempt_build(
                [{"role": "user", "content": "fix format X"}],
                call_model_fn=fake_call_model,
                git_apply_fn=lambda diff, root: (True, "ok"),
                git_checkout_clean_fn=lambda root: None,
                cargo_build_fn=lambda root: (True, ""),
                config=dict(CONFIG, max_request_turns=5),
                repo_root=repo,
            )
        # seen[0] is the original prompt; seen[1..] are REQUEST answers.
        self.assertIn("(investigation turn 1 of 5 -- 4 left)", seen[1])
        self.assertIn("(investigation turn 2 of 5 -- 3 left)", seen[2])
        # ...and the counter must be the LAST thing in the message: it changes
        # every turn, so anywhere but the tail it would break the provider's
        # cached prefix for every later call in the attempt.
        self.assertTrue(seen[1].rstrip().endswith("4 left)"), seen[1])
        self.assertIn("fn a() {}", seen[1])   # the answer itself still comes first

    def test_final_investigation_turn_is_told_so_before_it_is_spent(self):
        answers = []

        def fake_call_model(messages, *a):
            if len(answers) < 2:
                answers.append(messages[-1]["content"])
                return "REQUEST: src/x.rs"
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "x.rs").write_text("fn a() {}\n")
            attempt_build(
                [{"role": "user", "content": "fix format X"}],
                call_model_fn=fake_call_model,
                git_apply_fn=lambda diff, root: (True, "ok"),
                git_checkout_clean_fn=lambda root: None,
                cargo_build_fn=lambda root: (True, ""),
                config=dict(CONFIG, max_request_turns=1),
                repo_root=repo,
            )
        final = answers[1]   # the answer to the one and only allowed REQUEST
        self.assertIn("this was your LAST", final)
        self.assertIn("another REQUEST will be discarded", final)
        self.assertIn("best-effort diff", final)

    def test_budget_footer_wording(self):
        self.assertEqual(render_request_budget_footer(3, 5),
                         "(investigation turn 3 of 5 -- 2 left)")
        last = render_request_budget_footer(5, 5)
        self.assertIn("this was your LAST", last)
        self.assertNotIn("2 left", last)

    def test_dead_end_pivot_message_also_carries_the_budget(self):
        # The max_request_repeats dead-end branch is still a consumed
        # investigation turn, so it must show the same counter.
        seen = []

        def fake_call_model(messages, *a):
            seen.append(messages[-1]["content"])
            if len(seen) <= 4:
                return "REQUEST: src/x.rs"
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "x.rs").write_text("fn a() {}\n")
            attempt_build(
                [{"role": "user", "content": "fix format X"}],
                call_model_fn=fake_call_model,
                git_apply_fn=lambda diff, root: (True, "ok"),
                git_checkout_clean_fn=lambda root: None,
                cargo_build_fn=lambda root: (True, ""),
                config=dict(CONFIG, max_request_turns=8, max_request_repeats=3),
                repo_root=repo,
            )
        pivot = [m for m in seen if "requested 'src/x.rs' 3 times" in m]
        self.assertEqual(len(pivot), 1)
        self.assertIn("(investigation turn 3 of 8 -- 5 left)", pivot[0])

    def test_request_with_no_turns_left_forces_exactly_one_diff_only_retry(self):
        calls = []

        def fake_call_model(messages, *a):
            calls.append(messages[-1]["content"])
            return "REQUEST: src/parsers/jpeg/mod.rs"   # never gives up

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=dict(CONFIG, max_request_turns=2),
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertEqual(reason, "no diff in model response (exhausted request budget)")
        # 2 budgeted REQUEST turns + the one that overran + exactly ONE forced
        # retry = 4 calls. A 5th would mean the forced retry can loop.
        self.assertEqual(len(calls), 4)
        forced = [c for c in calls if c == FORCED_DIFF_DEMAND]
        self.assertEqual(len(forced), 1)
        # It must demand a diff and NOTHING else -- no "or a REQUEST if you
        # must" escape hatch, which is how the transcript kept investigating.
        self.assertIn("DISCARDED", FORCED_DIFF_DEMAND)
        self.assertIn("reply with a diff and nothing else", FORCED_DIFF_DEMAND)

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

    def test_table_phase_spec_s3_strongest_model(self):
        strongest = {"name": "strongest", "base_url": "u", "api_key": "k", "phase": "table",
                    "reasoning_effort": "max"}
        pool = [self.TERRA, self.SOL, strongest]
        self.assertEqual(models_for_phase(pool, "table"), [strongest])


class ModelSpecPhaseTests(unittest.TestCase):
    def test_phase_and_reasoning_effort_are_accepted(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k",
            "models": [{"name": "m", "phase": "explore", "reasoning_effort": "medium"}],
        })
        self.assertEqual(config["models"][0]["phase"], "explore")
        self.assertEqual(config["models"][0]["reasoning_effort"], "medium")

    def test_table_phase_is_accepted(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k",
            "models": [{"name": "m", "phase": "table"}],
        })
        self.assertEqual(config["models"][0]["phase"], "table")

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


class FixGapFailureTests(HermeticFixGapTestCase):
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
            git_commit_fn=lambda msg, root, **_kw: self.fail("should not commit"),
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
            git_commit_fn=lambda msg, root, **_kw: self.fail("should not commit"),
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


class FixGapTestOrderingTests(HermeticFixGapTestCase):
    def test_targeted_runs_before_review_full_suite_only_before_commit(self):
        order = []
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 0,
            cargo_test_targeted_fn=lambda root, f: (order.append("targeted"), (True, ""))[1],
            cargo_test_workspace_fn=lambda root: (order.append("full"), (True, ""))[1],
            review_fn=lambda *a, **k: (order.append("review"), (True, ""))[1],
            git_commit_fn=lambda msg, root, **_kw: order.append("commit"),
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
            git_commit_fn=lambda msg, root, **_kw: commits.append(1),
            git_checkout_clean_fn=lambda root: None,
            detect_duplicate_fn=lambda *a: False,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("full boom", result["reason"])
        self.assertEqual(commits, [])


class FixGapCritiqueTests(HermeticFixGapTestCase):
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: self.fail("should not commit"),
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
            git_commit_fn=lambda msg, root, **_kw: self.fail("should not commit"),
            repo_root=Path("/fake/repo"),
            max_repair_rounds=2,
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(critique_calls, [])
        self.assertEqual(len(result["rounds"]), 2)
        for r in result["rounds"]:
            self.assertEqual(r["critique"], infra_reason)


class FixGapReviewTests(HermeticFixGapTestCase):
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
            git_commit_fn=lambda msg, root, **_kw: commit_calls.append(msg),
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
            git_commit_fn=lambda msg, root, **_kw: self.fail("should not commit"),
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
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
            git_commit_fn=lambda msg, root, **_kw: None,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(seen_review_config[0], CONFIG)


class FixGapDuplicateDetectionTests(HermeticFixGapTestCase):
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
            git_commit_fn=lambda msg, root, **_kw: self.fail("must not commit a detected duplicate"),
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
            git_commit_fn=lambda msg, root, **_kw: commit_calls.append(msg),
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
            git_commit_fn=lambda msg, root, **_kw: None,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(detect_calls, [])


def _read_lessons(home):
    path = Path(home) / "logs" / "lessons.jsonl"
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


class FixGapK1LessonTests(HermeticFixGapTestCase):
    """Spec K1 writers: fix_gap best-effort appends a lesson event at
    every decision point. knowledge_home=None (every OTHER test in this
    file) is a documented no-op -- these tests opt in with a tempdir."""

    def test_build_failure_writes_build_failed_and_critique_events(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                make_gap(gap_count=2), CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (False, "compile error", None, messages),
                critique_fn=lambda *a, **k: "root cause: X",
                git_checkout_clean_fn=lambda root: None,
                repo_root=Path("/fake/repo"), max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e["event"] for e in _read_lessons(tmpdir)]
        self.assertIn("build_failed", events)
        self.assertIn("critique", events)

    def test_infra_failure_writes_infra_event_not_critique(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                make_gap(gap_count=2), CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (
                    False, "model call failed: HTTP Error 429", None, messages),
                git_checkout_clean_fn=lambda root: None,
                repo_root=Path("/fake/repo"), max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e["event"] for e in _read_lessons(tmpdir)]
        # Exactly ONE row, and it is infra. fix_gap used to ALSO write a
        # build_failed row carrying the same outage reason (from inside
        # `if not built:`, where a DNS error and a type error look
        # identical) -- 218 such duplicate rows on the live ledger
        # 2026-07-25, every one of them a knowledge-file bullet and a
        # line of some worker's prompt budget spent on a 429.
        self.assertEqual(events, ["infra"])
        self.assertNotIn("critique", events)

    def test_test_regression_writes_test_regressed_event(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                make_gap(gap_count=1), CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                recheck_fn=lambda fmt: 0,
                cargo_test_targeted_fn=lambda root, f: (False, "boom"),
                git_checkout_clean_fn=lambda root: None,
                critique_fn=lambda *a, **k: "critique",
                max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e["event"] for e in _read_lessons(tmpdir)]
        self.assertIn("test_regressed", events)

    def test_duplicate_writes_duplicate_event(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            gap = make_single_tag_gap_dict(source_file=None)
            fix_gap(
                gap, CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                detect_duplicate_fn=lambda *a: True,
                git_checkout_clean_fn=lambda root: None,
                cargo_test_targeted_fn=lambda root, f: (True, ""),
                recheck_fn=lambda fmt: 0,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = _read_lessons(tmpdir)
        self.assertEqual([e["event"] for e in events], ["duplicate"])

    def test_review_rejection_writes_review_rejected_with_checklist_id(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                make_gap(gap_count=2), CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                review_fn=lambda *a, **k: (False, "C2 paraphrased PrintConv"),
                cargo_test_targeted_fn=lambda root, f: (True, ""),
                git_checkout_clean_fn=lambda root: None,
                recheck_fn=lambda fmt: 0,
                max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e for e in _read_lessons(tmpdir) if e["event"] == "review_rejected"]
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["checklist_id"], "C2")

    def test_fixed_writes_fixed_event(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                make_gap(gap_count=2), CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                review_fn=lambda *a, **k: (True, ""),
                cargo_test_targeted_fn=lambda root, f: (True, ""),
                cargo_test_workspace_fn=lambda root: (True, ""),
                git_checkout_clean_fn=lambda root: None,
                git_commit_fn=lambda msg, root, **kw: None,
                recheck_fn=lambda fmt: 0,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e["event"] for e in _read_lessons(tmpdir)]
        self.assertEqual(events, ["fixed"])

    def test_lesson_write_failure_never_breaks_the_loop(self):
        # knowledge_home pointing at something unwritable (a file, not a
        # dir) must not raise out of fix_gap -- best-effort, per spec.
        with tempfile.TemporaryDirectory() as tmpdir:
            bad_home = Path(tmpdir) / "not_a_dir"
            bad_home.write_text("occupied")
            result = fix_gap(
                make_gap(gap_count=2), CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (False, "boom", None, messages),
                git_checkout_clean_fn=lambda root: None,
                repo_root=Path("/fake/repo"), max_repair_rounds=1,
                knowledge_home=bad_home, worker_label="w1",
            )
        self.assertEqual(result["status"], "failed")


class FixGapM3MultisetTests(HermeticFixGapTestCase):
    """Spec M3: fix_gap's recheck path classifies wrong_value/structural
    via tag_still_open when recheck_fn supplies a post-attempt
    comparison dict (the 3rd tuple element), and fails the attempt when
    new_oxidex_only_keys detects a newly-introduced oxidex-only tag."""

    def test_wrong_value_classification_from_post_match(self):
        gap = make_single_tag_gap_dict(source_file=None)  # APP0:OcadRevision
        post_match = {
            "missing_tags": [],
            "value_differences": [
                {"tag_key": "APP0:OcadRevision", "exiftool_value": "1", "oxidex_value": "2"},
            ],
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                gap, CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                recheck_fn=lambda fmt: (1, None, post_match),
                git_checkout_clean_fn=lambda root: None,
                critique_fn=lambda *a, **k: "critique",
                max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e for e in _read_lessons(tmpdir) if e["event"] == "wrong_value"]
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["evidence"], {"exiftool_value": "1", "oxidex_value": "2"})

    def test_duplicate_emission_classification_is_structural(self):
        gap = make_single_tag_gap_dict(source_file=None)
        post_match = {"missing_tags": [], "value_differences": [],
                      "duplicate_emissions": ["APP0:OcadRevision"]}
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                gap, CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                recheck_fn=lambda fmt: (1, None, post_match),
                git_checkout_clean_fn=lambda root: None,
                critique_fn=lambda *a, **k: "critique",
                max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e["event"] for e in _read_lessons(tmpdir)
                      if e["event"] in ("structural", "wrong_value", "gap_not_closed")]
        self.assertEqual(events, ["structural"])

    def test_legacy_2tuple_recheck_still_falls_back_to_gap_not_closed(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            fix_gap(
                make_gap(gap_count=1), CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                recheck_fn=lambda fmt: (1, "some detail"),
                git_checkout_clean_fn=lambda root: None,
                critique_fn=lambda *a, **k: "critique",
                max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e["event"] for e in _read_lessons(tmpdir)
                      if e["event"] in ("gap_not_closed", "wrong_value", "structural")]
        self.assertEqual(events, ["gap_not_closed"])

    def test_new_oxidex_only_tag_fails_the_attempt_and_logs_structural(self):
        gap = make_gap(gap_count=1)
        pre = {"extra_in_oxidex": []}
        post = {"missing_tags": [], "value_differences": [],
                "extra_in_oxidex": [{"family": "EXIF", "name": "Bogus"}]}
        with tempfile.TemporaryDirectory() as tmpdir:
            result = fix_gap(
                gap, CONFIG,
                attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
                recheck_fn=lambda fmt: (0, None, post),
                recheck_baseline=pre,
                git_checkout_clean_fn=lambda root: None,
                critique_fn=lambda *a, **k: "critique",
                max_repair_rounds=1,
                knowledge_home=tmpdir, worker_label="w1",
            )
            events = [e for e in _read_lessons(tmpdir) if e["event"] == "structural"]
        self.assertEqual(result["status"], "failed")
        self.assertIn("EXIF:Bogus", result["reason"])
        self.assertEqual(len(events), 1)

    def test_recheck_baseline_none_skips_the_new_oxidex_only_gate(self):
        # Even though post's extra_in_oxidex carries a tag, no
        # recheck_baseline means the gate simply isn't evaluated --
        # existing callers that never pass recheck_baseline (every other
        # test in this file) are unaffected.
        gap = make_gap(gap_count=1)
        post = {"missing_tags": [], "value_differences": [],
                "extra_in_oxidex": [{"family": "EXIF", "name": "Bogus"}]}
        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: (0, None, post),
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            review_fn=lambda *a, **k: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: None,
        )
        self.assertEqual(result["status"], "fixed")


class FixGapK5EvidenceTests(HermeticFixGapTestCase):
    """Spec K5: fix_gap threads perl_block/live_evidence/emission_scan
    into review_fn, folds UNVERIFIABLE into review_flags/trailers on
    C1/C2 only, and degrades evidence functions to "" on failure."""

    def test_evidence_fns_are_threaded_to_review_fn(self):
        gap = make_gap(gap_count=2)  # has source_file "a.nef"
        seen = {}

        def fake_extract_evidence(repo_root, sample_path, tag_keys):
            seen["sample_path"] = sample_path
            seen["tag_keys"] = list(tag_keys)
            return "EVIDENCE-TEXT"

        def fake_scan(repo_root, parser_files, tag_keys, diff_text):
            seen["scan_diff"] = diff_text
            return "SCAN-TEXT"

        def fake_review(g, diff, config, **kwargs):
            seen["perl_block"] = kwargs.get("perl_block")
            seen["live_evidence"] = kwargs.get("live_evidence")
            seen["emission_scan"] = kwargs.get("emission_scan")
            return True, ""

        fix_gap(
            gap, CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            review_fn=fake_review,
            extract_evidence_fn=fake_extract_evidence, scan_fn=fake_scan,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: None,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(seen["sample_path"], "a.nef")
        self.assertIn("EXIF:LensModel", seen["tag_keys"])
        self.assertEqual(seen["live_evidence"], "EVIDENCE-TEXT")
        self.assertEqual(seen["emission_scan"], "SCAN-TEXT")
        self.assertEqual(seen["scan_diff"], "--- a/x\n+++ b/x\n")

    def test_evidence_fn_exception_degrades_to_empty_string(self):
        def raising_evidence(*a, **k):
            raise TimeoutError("boom")

        seen = {}

        def fake_review(g, diff, config, **kwargs):
            seen["live_evidence"] = kwargs.get("live_evidence")
            return True, ""

        fix_gap(
            make_gap(gap_count=2), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            review_fn=fake_review,
            extract_evidence_fn=raising_evidence,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: None,
            recheck_fn=lambda fmt: 0,
        )
        self.assertEqual(seen["live_evidence"], "")

    def test_unverifiable_c1_sets_review_flags_and_trailer(self):
        commits = []

        def fake_git_commit(msg, root, trailers=None):
            commits.append(trailers)

        result = fix_gap(
            make_gap(gap_count=2), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            review_fn=lambda *a, **k: (True, "UNVERIFIABLE: C1 perl table not shown"),
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=fake_git_commit,
            recheck_fn=lambda fmt: 0,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(result["review_flags"], ["UNVERIFIABLE:C1"])
        trailer_dict = dict(commits[0])
        self.assertEqual(trailer_dict["Review-Unverifiable"], "UNVERIFIABLE:C1")

    def test_unverifiable_c3_does_not_set_review_flags(self):
        result = fix_gap(
            make_gap(gap_count=2), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            review_fn=lambda *a, **k: (True, "UNVERIFIABLE: C3 emission scan inconclusive"),
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: None,
            recheck_fn=lambda fmt: 0,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertNotIn("review_flags", result)

    def test_unverifiable_without_checklist_id_still_routes_to_human_queue(self):
        # A reviewer reply can say UNVERIFIABLE without a parseable
        # C1-C5 token (a plausible model formatting slip -- the prompt
        # requires one, but nothing enforces it). parse_checklist_id
        # returns None for this, and fix_gap must fail safe: unknown
        # severity still escalates to the human queue rather than
        # silently landing with no Review-Unverifiable trailer at all.
        commits = []

        def fake_git_commit(msg, root, trailers=None):
            commits.append(trailers)

        result = fix_gap(
            make_gap(gap_count=2), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            review_fn=lambda *a, **k: (
                True, "UNVERIFIABLE: the perl table didn't fit the prompt"),
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=fake_git_commit,
            recheck_fn=lambda fmt: 0,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(result["review_flags"], ["UNVERIFIABLE:UNKNOWN"])
        trailer_dict = dict(commits[0])
        self.assertEqual(trailer_dict["Review-Unverifiable"], "UNVERIFIABLE:UNKNOWN")

    def test_unverifiable_with_no_reason_at_all_still_routes_to_human_queue(self):
        # extract_review_verdict_full's own "no checklist id given"
        # fallback text (for a bare "UNVERIFIABLE" with nothing after
        # the colon) must also escalate, not just a reply with prose but
        # no Cn.
        result = fix_gap(
            make_gap(gap_count=2), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            review_fn=lambda *a, **k: (True, "UNVERIFIABLE: unverifiable, no checklist id given"),
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: None,
            recheck_fn=lambda fmt: 0,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(result["review_flags"], ["UNVERIFIABLE:UNKNOWN"])


class DefaultExtractLiveEvidenceTests(unittest.TestCase):
    def test_returns_empty_when_no_sample_path(self):
        from model_fix_loop import default_extract_live_evidence
        self.assertEqual(default_extract_live_evidence(Path("/fake"), None, ["EXIF:Make"]), "")

    def test_returns_empty_when_no_binary_built(self):
        from model_fix_loop import default_extract_live_evidence
        with tempfile.TemporaryDirectory() as tmpdir:
            self.assertEqual(
                default_extract_live_evidence(Path(tmpdir), "sample.jpg", ["EXIF:Make"]), "")

    @patch("model_fix_loop.shutil.which", return_value="/usr/bin/exiftool")
    @patch("model_fix_loop.subprocess.run")
    def test_renders_matched_tag_values(self, mock_run, mock_which):
        from model_fix_loop import default_extract_live_evidence
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            binary_dir = repo / "target" / "debug"
            binary_dir.mkdir(parents=True)
            (binary_dir / "oxidex").write_text("#!/bin/sh\n")
            mock_run.return_value = MagicMock(stdout='[{"EXIF:Make": "Canon"}]')
            result = default_extract_live_evidence(repo, "sample.jpg", ["EXIF:Make"])
        self.assertIn("EXIF:Make", result)
        self.assertIn("Canon", result)


class DefaultEmissionScanTests(unittest.TestCase):
    def test_empty_when_no_tag_keys(self):
        from model_fix_loop import default_emission_scan
        self.assertEqual(default_emission_scan(Path("/fake"), ["src/x.rs"], []), "")

    def test_empty_when_no_parser_files_and_no_diff(self):
        from model_fix_loop import default_emission_scan
        self.assertEqual(default_emission_scan(Path("/fake"), [], ["EXIF:Make"]), "")

    def test_diff_pre_post_counts_included(self):
        from model_fix_loop import default_emission_scan
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "x.rs").write_text(
                'insert("EXIF:Make", v); insert("EXIF:Make", v2);'
            )
            diff = "+++ b/src/x.rs\n"
            result = default_emission_scan(repo, [], ["EXIF:Make"], diff_text=diff)
        self.assertIn("pre=", result)
        self.assertIn("post=", result)


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

    def test_duplicate_emission_is_still_open_even_though_otherwise_closed(self):
        # Spec M3: a tag emitted twice for the same sample file must
        # never pass recheck, even though it's absent from BOTH
        # missing_tags and value_differences (a HashMap-backed
        # MetadataMap can only hold one value per key, so it "looks"
        # closed by the old two-list check alone).
        match = {"missing_tags": [], "value_differences": [],
                 "duplicate_emissions": ["XMP:ArtworkTitle"]}
        self.assertEqual(tag_still_open(match, self.MISSING_GAP), ("duplicate_emission",))

    def test_duplicate_emission_for_diff_kind_gap(self):
        match = {"missing_tags": [], "value_differences": [],
                 "duplicate_emissions": ["EXIF:ISO"]}
        self.assertEqual(tag_still_open(match, self.DIFF_GAP), ("duplicate_emission",))

    def test_duplicate_emissions_of_unrelated_tags_dont_affect_verdict(self):
        match = {"missing_tags": [], "value_differences": [],
                 "duplicate_emissions": ["EXIF:SomeOtherTag"]}
        self.assertIsNone(tag_still_open(match, self.MISSING_GAP))


class NewlyDuplicatedEmissionsTests(unittest.TestCase):
    """The other half of the same gate, which was reading POST directly.

    Measured 2026-07-27: NEF carries NINE duplicate_emissions on clean main
    (EXIF:BitsPerSample, Compression, ImageHeight, ImageWidth,
    PhotometricInterpretation, RowsPerStrip, SamplesPerPixel, StripOffsets,
    SubfileType), so every NEF commit was quarantined for inheriting them --
    including d8168e7b, which introduced none. A commit is answerable for
    the duplicates it INTRODUCES, never for the ones it inherits.
    """

    def test_pre_existing_duplicates_are_NOT_blamed_on_the_commit(self):
        nine = ["EXIF:BitsPerSample", "EXIF:Compression", "EXIF:ImageHeight"]
        pre = {"duplicate_emissions": nine}
        post = {"duplicate_emissions": nine}
        self.assertEqual(newly_duplicated_emissions(pre, post), [])

    def test_an_introduced_duplicate_is_reported(self):
        pre = {"duplicate_emissions": ["EXIF:Compression"]}
        post = {"duplicate_emissions": ["EXIF:Compression", "GIF:BackgroundColor"]}
        self.assertEqual(newly_duplicated_emissions(pre, post), ["GIF:BackgroundColor"])

    def test_multiple_introduced_are_sorted(self):
        pre = {"duplicate_emissions": []}
        post = {"duplicate_emissions": ["MakerNotes:Z", "EXIF:A"]}
        self.assertEqual(newly_duplicated_emissions(pre, post), ["EXIF:A", "MakerNotes:Z"])

    def test_a_duplicate_the_commit_REMOVED_is_not_reported(self):
        pre = {"duplicate_emissions": ["EXIF:A"]}
        post = {"duplicate_emissions": []}
        self.assertEqual(newly_duplicated_emissions(pre, post), [])

    def test_missing_reports_are_treated_as_empty(self):
        self.assertEqual(newly_duplicated_emissions(None, None), [])


class NewOxidexOnlyKeysTests(unittest.TestCase):
    """Spec M3: sorted extra_in_oxidex keys present post but not pre --
    the recheck path's structural double-emission gate (a fix that
    introduces a NEW oxidex-only tag as a side effect)."""

    def test_no_new_keys_when_extra_in_oxidex_unchanged(self):
        pre = {"extra_in_oxidex": [{"family": "EXIF", "name": "A"}]}
        post = {"extra_in_oxidex": [{"family": "EXIF", "name": "A"}]}
        self.assertEqual(new_oxidex_only_keys(pre, post), [])

    def test_new_key_detected(self):
        pre = {"extra_in_oxidex": []}
        post = {"extra_in_oxidex": [{"family": "EXIF", "name": "Bogus"}]}
        self.assertEqual(new_oxidex_only_keys(pre, post), ["EXIF:Bogus"])

    def test_multiple_new_keys_sorted(self):
        pre = {"extra_in_oxidex": []}
        post = {"extra_in_oxidex": [
            {"family": "MakerNotes", "name": "Z"}, {"family": "EXIF", "name": "A"},
        ]}
        self.assertEqual(new_oxidex_only_keys(pre, post), ["EXIF:A", "MakerNotes:Z"])

    def test_a_key_removed_is_not_reported_as_new(self):
        pre = {"extra_in_oxidex": [{"family": "EXIF", "name": "A"}]}
        post = {"extra_in_oxidex": []}
        self.assertEqual(new_oxidex_only_keys(pre, post), [])

    def test_missing_reports_are_treated_as_empty(self):
        self.assertEqual(new_oxidex_only_keys(None, None), [])
        self.assertEqual(new_oxidex_only_keys({}, {"extra_in_oxidex": [{"family": "A", "name": "B"}]}),
                         ["A:B"])


class FixGapRecheckDetailTests(HermeticFixGapTestCase):
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

    def test_reverted_tombstone_removes_tag_from_landed_set(self):
        # Spec M5: log_sweep_review.py --revert appends a
        # "<ts> REVERTED <tag_key>" tombstone; the reverted tag must
        # re-enter the worker pool, not stay suppressed forever (and the
        # tombstone line itself must never become a junk landed key).
        with tempfile.TemporaryDirectory() as tmpdir:
            p = Path(tmpdir) / "landed.log"
            p.write_text(
                "2026-07-23T17:00:00 JPEG:MakerNotes:AELButton\n"
                "2026-07-23T17:05:00 PSD:EXIF:Compression\n"
                "2026-07-24T09:00:00 REVERTED JPEG:MakerNotes:AELButton\n")
            self.assertEqual(load_landed_tags(p), {"PSD:EXIF:Compression"})

    def test_reland_after_revert_rejoins_skip_set_in_file_order(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            p = Path(tmpdir) / "landed.log"
            p.write_text(
                "2026-07-23T17:00:00 JPEG:APP12:MODE3\n"
                "2026-07-24T09:00:00 REVERTED JPEG:APP12:MODE3\n"
                "2026-07-24T11:00:00 JPEG:APP12:MODE3\n")
            self.assertEqual(load_landed_tags(p), {"JPEG:APP12:MODE3"})

    def test_tombstone_for_never_landed_tag_is_harmless(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            p = Path(tmpdir) / "landed.log"
            p.write_text("2026-07-24T09:00:00 REVERTED NEF:EXIF:NeverLanded\n")
            self.assertEqual(load_landed_tags(p), set())


class LoadSaveTagStateTests(unittest.TestCase):
    def setUp(self):
        self._tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmpdir.cleanup)
        self.path = Path(self._tmpdir.name) / "model-fix-tag-state.json"

    def test_missing_file_is_an_empty_state(self):
        self.assertEqual(load_tag_state(self.path), {})

    def test_save_then_load_round_trips(self):
        save_tag_state(self.path, {"NEF:EXIF:ISO": {"fails": 3, "blacklisted": False}})
        self.assertEqual(load_tag_state(self.path), {"NEF:EXIF:ISO": {"fails": 3, "blacklisted": False}})

    def test_save_leaves_no_temp_file_behind(self):
        # tempfile+os.replace: the directory holds exactly the state
        # file afterward, never an orphaned .tmp sibling a reader could
        # trip over.
        save_tag_state(self.path, {"a": {}})
        save_tag_state(self.path, {"a": {}, "b": {}})
        self.assertEqual([p.name for p in self.path.parent.iterdir()], [self.path.name])

    def test_torn_file_raises_instead_of_returning_empty(self):
        # The old permissive behavior (torn read -> {}) let the next
        # save wipe every worker's entries. A torn read must now stop
        # the run.
        self.path.write_text('{"NEF:EXIF:ISO": {"fails"')
        with self.assertRaises(ValueError):
            load_tag_state(self.path)

    def test_non_dict_json_raises(self):
        self.path.write_text('["not", "a", "dict"]')
        with self.assertRaises(ValueError):
            load_tag_state(self.path)


def _state_contender(state_path, worker_id, n_ops, results_path):
    """Child-process body for StateLockedContentionTests -- module-level
    so multiprocessing's spawn start method can import it by name. Each
    of n_ops iterations does one full locked read-modify-write:
    increment a shared counter and claim the first unclaimed slot."""
    claimed = []
    for _ in range(n_ops):
        def mutate(state):
            state["counter"] = state.get("counter", 0) + 1
            for key in sorted(state["slots"]):
                if state["slots"][key] is None:
                    state["slots"][key] = worker_id
                    claimed.append(key)
                    break
            return state, None

        _state_locked(state_path, mutate)
    Path(results_path).write_text(json.dumps(claimed))


class StateLockedContentionTests(unittest.TestCase):
    def test_two_processes_claiming_under_contention_lose_nothing(self):
        # Two real processes hammer one state file through _state_locked.
        # Without the flock this reliably loses updates (load/save pairs
        # interleave); with it, every increment lands and every slot has
        # exactly one owner.
        with tempfile.TemporaryDirectory() as tmpdir:
            state_path = str(Path(tmpdir) / "state.json")
            n_ops = 20
            save_tag_state(state_path, {
                "counter": 0,
                "slots": {f"slot{i:02d}": None for i in range(2 * n_ops)},
            })
            ctx = multiprocessing.get_context("spawn")
            results = {w: str(Path(tmpdir) / f"claims-{w}.json") for w in ("a", "b")}
            procs = [
                ctx.Process(target=_state_contender, args=(state_path, w, n_ops, results[w]))
                for w in ("a", "b")
            ]
            for p in procs:
                p.start()
            for p in procs:
                p.join(timeout=120)
                self.assertEqual(p.exitcode, 0)

            final = load_tag_state(state_path)
            # No lost update: all 2*n_ops locked increments landed.
            self.assertEqual(final["counter"], 2 * n_ops)
            claims_a = set(json.loads(Path(results["a"]).read_text()))
            claims_b = set(json.loads(Path(results["b"]).read_text()))
            # Disjoint claims: no slot was handed to both processes.
            self.assertEqual(claims_a & claims_b, set())
            self.assertEqual(len(claims_a) + len(claims_b), 2 * n_ops)
            # And the persisted state agrees with each claimant's record.
            for key, owner in final["slots"].items():
                self.assertIn(key, claims_a if owner == "a" else claims_b)


class RunTagLoopTests(unittest.TestCase):
    def setUp(self):
        # run_tag_loop serializes every state access through a real
        # flock on state_path's sibling .lock file even when
        # load/save_state_fn are injected in-memory fakes -- so the
        # state path must live somewhere a lock file can actually be
        # created.
        self._tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmpdir.cleanup)
        self.state_path = str(Path(self._tmpdir.name) / "state.json")

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
            state_path=self.state_path,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
            max_rounds=2,
        )
        # Round 1: everything blacklisted -> reset (no attempt made).
        # Round 2: blacklist is empty again -> one of the two tags gets a
        # fresh attempt.
        self.assertEqual(result["cycles_reset"], 1)
        self.assertEqual(len(attempts), 1)

    def test_exhaustion_reset_deletes_exactly_the_considered_keys(self):
        # The reset is scoped to the explicit key list this worker
        # considered at claim-filter time (its own format's tags) --
        # entries belonging to other formats/workers in the same shared
        # state file must survive untouched. The old `state = {}` wiped
        # them all, including live claims.
        gaps = [make_gap()]  # this worker considers NEF:EXIF:LensModel + NEF:EXIF:ISO

        store, load, save = self._state_io()
        store["NEF:EXIF:LensModel"] = {"fails": 2, "blacklisted": True}
        store["NEF:EXIF:ISO"] = {"fails": 2, "blacklisted": True}
        # Another format's blacklist entry and another worker's live
        # claim, both sharing this state file:
        store["XMP:XMP:Title"] = {"fails": 10, "blacklisted": True}
        store["JPEG:APP12:Qualite"] = {
            "fails": 0, "blacklisted": False, "attempts": [],
            "claimed_by": "jpeg-worker", "claimed_at": time.time(),
        }

        result = run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps,
            fix_gap_fn=lambda tg, c, previous_attempts=None: self.fail("nothing claimable this round"),
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
            max_rounds=1,
        )
        self.assertEqual(result["cycles_reset"], 1)
        self.assertNotIn("NEF:EXIF:LensModel", store)
        self.assertNotIn("NEF:EXIF:ISO", store)
        self.assertIn("XMP:XMP:Title", store)
        self.assertIn("JPEG:APP12:Qualite", store)
        self.assertEqual(store["JPEG:APP12:Qualite"]["claimed_by"], "jpeg-worker")

    def test_torn_state_file_raises_instead_of_wiping(self):
        # Acceptance criterion (spec Phase 0): a torn tag-state file must
        # stop the loop, with the file left byte-for-byte intact -- never
        # be silently treated as {} and then overwritten.
        torn = '{"NEF:EXIF:LensModel": {"fails"'
        Path(self.state_path).write_text(torn)
        with self.assertRaises(ValueError):
            run_tag_loop(
                {"models": ["x"]}, find_gaps_fn=lambda: [make_gap()],
                fix_gap_fn=lambda tg, c, previous_attempts=None: {"status": "failed", "reason": "n"},
                state_path=self.state_path,  # default (real) load/save fns
                max_rounds=1,
            )
        self.assertEqual(Path(self.state_path).read_text(), torn)

    def test_heartbeat_keeps_a_slow_claim_alive_past_the_stale_threshold(self):
        # Fake clock: the claim is stamped at t=1000, then the attempt
        # "runs" long enough that the clock reads t=3000 -- 2000 fake
        # seconds, past claim_stale_seconds=1800, so without a heartbeat
        # any other worker would treat the claim as abandoned. The
        # heartbeat thread (real thread, tiny real cadence, fake
        # timestamps) re-stamps claimed_at with the advanced clock, so
        # the claim reads fresh for as long as the attempt lives.
        clock = [1000.0]
        observed = {}

        def fake_fix(tag_gap, config, previous_attempts=None):
            clock[0] = 3000.0
            deadline = time.time() + 10
            while time.time() < deadline:
                entry = load_tag_state(self.state_path).get(tag_gap["tag_key"]) or {}
                if entry.get("claimed_at") == 3000.0:
                    observed.update(entry)
                    break
                time.sleep(0.005)
            return {"status": "failed", "reason": "nope"}

        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: [make_gap()], fix_gap_fn=fake_fix,
            state_path=self.state_path,  # default (real) load/save fns
            max_rounds=1, worker_id="w1", claim_stale_seconds=1800,
            heartbeat_seconds=0.01, time_fn=lambda: clock[0],
        )
        self.assertEqual(observed.get("claimed_by"), "w1")
        self.assertEqual(observed.get("claimed_at"), 3000.0)
        # The staleness math another worker would run mid-attempt: the
        # original stamp (t=1000) would read abandoned; the heartbeat's
        # re-stamp does not.
        self.assertGreaterEqual(3000.0 - 1000.0, 1800)  # un-heartbeated claim would be stale
        self.assertLess(3000.0 - observed["claimed_at"], 1800)  # heartbeated claim is fresh

    def test_heartbeat_thread_stops_when_the_attempt_ends(self):
        # After run_tag_loop returns, no claim-heartbeat thread may
        # still be running (threading.Event stop + join).
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: [make_gap()],
            fix_gap_fn=lambda tg, c, previous_attempts=None: {"status": "failed", "reason": "n"},
            state_path=self.state_path,
            max_rounds=1, worker_id="w1", heartbeat_seconds=0.01,
        )
        self.assertFalse(
            [t.name for t in threading.enumerate() if t.name.startswith("claim-heartbeat-")]
        )

    def test_heartbeat_survives_a_transient_touch_failure(self):
        # A heartbeat touch can legitimately raise mid-attempt --
        # load_tag_state's raise-on-torn-read while a pre-flock worker
        # is still writing the file non-atomically (the mixed-version
        # rollout window), or ENOSPC/EACCES on save. One such raise must
        # not kill the daemon thread for the rest of a multi-hour
        # attempt (silently reverting to the stale-claim/double-claim
        # behavior the heartbeat exists to prevent): the loop logs via
        # log_fn and beats again. Proven end-to-end here: the FIRST
        # touch from the heartbeat thread raises, and the attempt then
        # observes a re-stamp at the advanced clock -- a beat that can
        # only have come from the thread surviving its failure.
        clock = [1000.0]
        heartbeat_failed = threading.Event()
        logged = []
        observed = {}

        def flaky_load(path):
            # Only the heartbeat thread's loads fail (once); the main
            # loop's claim/record path must stay healthy throughout.
            if (threading.current_thread().name.startswith("claim-heartbeat-")
                    and not heartbeat_failed.is_set()):
                heartbeat_failed.set()
                raise ValueError("synthetic torn tag-state read")
            return load_tag_state(path)

        def fake_fix(tag_gap, config, previous_attempts=None):
            clock[0] = 3000.0
            deadline = time.time() + 10
            while time.time() < deadline:
                if heartbeat_failed.is_set():
                    entry = load_tag_state(self.state_path).get(tag_gap["tag_key"]) or {}
                    if entry.get("claimed_at") == 3000.0:
                        observed.update(entry)
                        break
                time.sleep(0.005)
            return {"status": "failed", "reason": "nope"}

        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: [make_gap()], fix_gap_fn=fake_fix,
            state_path=self.state_path, load_state_fn=flaky_load, log_fn=logged.append,
            max_rounds=1, worker_id="w1", claim_stale_seconds=1800,
            heartbeat_seconds=0.01, time_fn=lambda: clock[0],
        )
        self.assertTrue(heartbeat_failed.is_set())
        self.assertEqual(observed.get("claimed_by"), "w1")
        # The post-failure beat landed: the claim reads fresh at t=3000
        # even though one touch blew up along the way.
        self.assertEqual(observed.get("claimed_at"), 3000.0)
        self.assertTrue(any("heartbeat touch failed" in line for line in logged))

    def test_fixed_tag_clears_its_state_entry(self):
        gaps = [make_gap()]

        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "fixed", "gaps_closed": 1}

        store, load, save = self._state_io()
        store["NEF:EXIF:LensModel"] = {"fails": 1, "blacklisted": False}
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path,
            load_state_fn=lambda p: {},
            save_state_fn=lambda p, s: written.append((p, dict(s))),
            max_rounds=1,
        )
        self.assertEqual(written[-1][0], Path(self.state_path))
        self.assertIn("NEF:EXIF:LensModel", written[-1][1])

    def test_calls_git_checkout_clean_only_when_a_tag_gets_blacklisted(self):
        gaps = [make_gap()]
        clean_calls = []

        def fake_fix(tag_gap, config, previous_attempts=None):
            return {"status": "failed", "reason": "nope"}

        store, load, save = self._state_io()
        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
            git_checkout_clean_fn=lambda root: clean_calls.append(root),
            repo_root=Path("/fake/repo"),
            max_rounds=1, max_fails=2,
        )
        # First failure only -- not blacklisted yet, so no cleanup call.
        self.assertEqual(clean_calls, [])

        run_tag_loop(
            {"models": ["x"]}, find_gaps_fn=lambda: gaps, fix_gap_fn=fake_fix,
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
                state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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

    setUp = RunTagLoopTests.setUp
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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
            state_path=self.state_path, load_state_fn=load, save_state_fn=save,
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


# =============================================================================
# Phase 4/5: T3 TABLE-PORT / T4 FOUNDATION-UNLOCK job tiers, cross-tier claim
# exclusion, build semaphore -- spec S3, S4 item 5, section 5.
# =============================================================================

FOUNDATION_JOBS_TOML_PATH = Path(__file__).resolve().parent / "foundation_jobs.toml"


class LoadFoundationJobsTests(unittest.TestCase):
    def test_the_checked_in_seed_file_parses_to_exactly_seven_jobs(self):
        jobs = load_foundation_jobs(FOUNDATION_JOBS_TOML_PATH)
        self.assertEqual(len(jobs), 7)

    def test_every_job_has_the_required_fields(self):
        jobs = load_foundation_jobs(FOUNDATION_JOBS_TOML_PATH)
        for job in jobs:
            for field in ("name", "description", "target_formats", "target_module", "estimated_gaps", "status"):
                self.assertIn(field, job, f"job {job.get('name')!r} missing {field!r}")
            self.assertIsInstance(job["target_formats"], list)
            self.assertTrue(job["target_formats"])
            self.assertIsInstance(job["estimated_gaps"], int)

    def test_status_defaults_to_pending(self):
        jobs = load_foundation_jobs(FOUNDATION_JOBS_TOML_PATH)
        for job in jobs:
            self.assertEqual(job["status"], "pending")

    def test_names_are_unique(self):
        jobs = load_foundation_jobs(FOUNDATION_JOBS_TOML_PATH)
        names = [j["name"] for j in jobs]
        self.assertEqual(len(names), len(set(names)))

    def test_missing_required_field_raises(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "foundation_jobs.toml"
            path.write_text('[[jobs]]\nname = "x"\ndescription = "d"\n')
            with self.assertRaises(ValueError):
                load_foundation_jobs(path)


PERL_TABLE_SOURCE_FIXTURE = """package Image::ExifTool::Canon;

%Image::ExifTool::Canon::CameraSettings = (
    %binaryDataAttrs,
    FORMAT => 'int16s',
    FIRST_ENTRY => 1,
    GROUPS => { 0 => 'MakerNotes', 2 => 'Camera' },
    1 => {
        Name => 'MacroMode',
        PrintConv => {
            1 => 'Macro',
            2 => 'Normal',
        },
    },
    2 => { Name => 'SelfTimer' },
);

%Image::ExifTool::Canon::ShotInfo = (
    GROUPS => { 0 => 'MakerNotes', 2 => 'Camera' },
    1 => { Name => 'AutoISO' },
);
"""


class ExtractPerlTableSourceTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.lib_dir = Path(self.tmpdir.name)
        (self.lib_dir / "Canon.pm").write_text(PERL_TABLE_SOURCE_FIXTURE)

    def test_returns_none_when_lib_dir_is_none(self):
        self.assertIsNone(extract_perl_table_source("Canon::CameraSettings", None))

    def test_returns_none_when_table_not_found(self):
        self.assertIsNone(extract_perl_table_source("Canon::NoSuchTable", self.lib_dir))

    def test_extracts_the_complete_table_not_just_one_tag(self):
        source = extract_perl_table_source("Canon::CameraSettings", self.lib_dir)
        self.assertIsNotNone(source)
        self.assertIn("MacroMode", source)
        self.assertIn("SelfTimer", source)
        # Must not bleed into the NEXT table's own members.
        self.assertNotIn("AutoISO", source)
        # Nested hash (PrintConv) must be included, not truncated at the
        # first inner "}".
        self.assertIn("'Normal'", source)

    def test_stops_at_the_tables_own_closing_paren(self):
        source = extract_perl_table_source("Canon::ShotInfo", self.lib_dir)
        self.assertIn("AutoISO", source)
        self.assertNotIn("MacroMode", source)

    def test_truncates_when_over_max_chars(self):
        source = extract_perl_table_source("Canon::CameraSettings", self.lib_dir, max_chars=20)
        self.assertIn("truncated", source)


RUST_REGISTRY_FIXTURE = """//! Canon tag registry with array schemas

static CAMERA_SETTINGS_SCHEMA: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE),
        ArrayIndexDef::raw(2, "SelfTimer"),
    ],
};

static SHOT_INFO_SCHEMA: ArraySchema = ArraySchema {
    name: "ShotInfo",
    indices: &[
        ArrayIndexDef::raw(1, "AutoISO"),
    ],
};
"""


class BuildTablePortRegistrySkeletonTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.repo_root = Path(self.tmpdir.name)
        registries_dir = self.repo_root / "src" / "parsers" / "tiff" / "makernotes" / "registries"
        registries_dir.mkdir(parents=True)
        (registries_dir / "canon.rs").write_text(RUST_REGISTRY_FIXTURE)

    def test_matches_by_table_name_field(self):
        skeleton = build_table_port_registry_skeleton("Canon", "Canon::CameraSettings", self.repo_root)
        self.assertIsNotNone(skeleton)
        self.assertIn("CAMERA_SETTINGS_SCHEMA", skeleton)
        self.assertIn("MacroMode", skeleton)
        self.assertNotIn("AutoISO", skeleton)

    def test_labels_itself_scaffolding_only(self):
        skeleton = build_table_port_registry_skeleton("Canon", "Canon::CameraSettings", self.repo_root)
        self.assertIn("SCAFFOLDING ONLY", skeleton)

    def test_short_table_name_without_module_prefix_also_matches(self):
        skeleton = build_table_port_registry_skeleton("Canon", "ShotInfo", self.repo_root)
        self.assertIsNotNone(skeleton)
        self.assertIn("AutoISO", skeleton)

    def test_returns_none_when_module_file_missing(self):
        self.assertIsNone(build_table_port_registry_skeleton("Nikon", "Nikon::ShotInfo", self.repo_root))

    def test_returns_none_when_no_module_given(self):
        self.assertIsNone(build_table_port_registry_skeleton(None, "Canon::CameraSettings", self.repo_root))

    def test_returns_none_when_no_schema_matches(self):
        self.assertIsNone(build_table_port_registry_skeleton("Canon", "Canon::NoSuchTable", self.repo_root))


class EvaluateTablePortGateTests(unittest.TestCase):
    """Spec S3's three-clause gate -- clause (a) exact-ratio threshold,
    (b) zero regressions of previously-exact members, (c) zero
    present-but-wrong members -- and their interaction."""

    MEMBERS = ["Canon:A", "Canon:B", "Canon:C", "Canon:D", "Canon:E"]

    def _report(self, missing=(), wrong=()):
        return {
            "missing_tags": [{"family": "Canon", "name": n.split(":")[1]} for n in missing],
            "value_differences": [{"tag_key": n} for n in wrong],
        }

    def test_all_exact_passes(self):
        pre = self._report(missing=self.MEMBERS)
        post = self._report()
        passed, reason, must_remove = evaluate_table_port_gate(pre, post, self.MEMBERS)
        self.assertTrue(passed)
        self.assertEqual(must_remove, [])
        self.assertIn("5/5", reason)

    def test_exactly_at_threshold_passes(self):
        pre = self._report(missing=self.MEMBERS)
        post = self._report(missing=["Canon:E"])  # 4/5 = 0.8
        passed, reason, must_remove = evaluate_table_port_gate(pre, post, self.MEMBERS, threshold=0.8)
        self.assertTrue(passed)

    def test_one_below_threshold_fails_clause_a(self):
        pre = self._report(missing=self.MEMBERS)
        post = self._report(missing=["Canon:D", "Canon:E"])  # 3/5 = 0.6
        passed, reason, must_remove = evaluate_table_port_gate(pre, post, self.MEMBERS, threshold=0.8)
        self.assertFalse(passed)
        self.assertIn("clause a", reason)
        self.assertEqual(must_remove, [])

    def test_regression_of_a_previously_exact_member_fails_clause_b(self):
        pre = self._report()  # everything exact pre-attempt
        post = self._report(missing=["Canon:A"])
        passed, reason, must_remove = evaluate_table_port_gate(pre, post, self.MEMBERS)
        self.assertFalse(passed)
        self.assertIn("clause b", reason)
        self.assertIn("Canon:A", reason)
        self.assertEqual(must_remove, [])

    def test_member_newly_wrong_that_was_previously_missing_is_not_a_regression(self):
        """Spec's own called-out edge case: a member that goes
        missing -> wrong is caught by clause (c), NOT flagged as a
        clause-(b) regression (it was never matching to begin with)."""
        pre = self._report(missing=self.MEMBERS)
        post = self._report(wrong=["Canon:A"])
        passed, reason, must_remove = evaluate_table_port_gate(pre, post, self.MEMBERS)
        self.assertFalse(passed)
        self.assertNotIn("clause b", reason)
        self.assertIn("clause c", reason)
        self.assertEqual(must_remove, ["Canon:A"])

    def test_present_but_wrong_fails_clause_c_even_with_high_ratio(self):
        pre = self._report(missing=self.MEMBERS)
        post = self._report(wrong=["Canon:E"])  # 4/5 exact, but one is wrong not missing
        passed, reason, must_remove = evaluate_table_port_gate(pre, post, self.MEMBERS, threshold=0.5)
        self.assertFalse(passed)
        self.assertIn("clause c", reason)
        self.assertEqual(must_remove, ["Canon:E"])

    def test_member_that_regressed_and_is_present_but_wrong_trips_both_clauses(self):
        pre = self._report()  # all exact pre
        post = self._report(wrong=["Canon:A"])
        passed, reason, must_remove = evaluate_table_port_gate(pre, post, self.MEMBERS)
        self.assertFalse(passed)
        self.assertIn("clause b", reason)
        self.assertIn("clause c", reason)
        self.assertEqual(must_remove, ["Canon:A"])

    def test_no_table_members_given_fails(self):
        passed, reason, must_remove = evaluate_table_port_gate({}, {}, [])
        self.assertFalse(passed)
        self.assertIn("no table members", reason)


class ResolveCanonicalTableTests(unittest.TestCase):
    def test_none_attribution_resolves_to_none(self):
        self.assertEqual(resolve_canonical_table("JPEG:MakerNotes:Foo", None), (None, None))

    def test_missing_key_resolves_to_none(self):
        attribution = {"tags": {}}
        self.assertEqual(resolve_canonical_table("JPEG:MakerNotes:Foo", attribution), (None, None))

    def test_unknown_module_resolves_to_none(self):
        attribution = {"tags": {"JPEG:MakerNotes:Foo": {"module": "unknown", "table": ""}}}
        self.assertEqual(resolve_canonical_table("JPEG:MakerNotes:Foo", attribution), (None, None))

    def test_resolves_module_and_table(self):
        attribution = {"tags": {"JPEG:MakerNotes:Foo": {"module": "Canon", "table": "CameraSettings"}}}
        module, table = resolve_canonical_table("JPEG:MakerNotes:Foo", attribution)
        self.assertEqual(module, "Canon")
        self.assertEqual(table, "Canon::CameraSettings")

    def test_resolves_module_with_blank_table(self):
        attribution = {"tags": {"JPEG:MakerNotes:Foo": {"module": "Canon", "table": ""}}}
        module, table = resolve_canonical_table("JPEG:MakerNotes:Foo", attribution)
        self.assertEqual(module, "Canon")
        self.assertIsNone(table)


class ClaimConflictsTests(unittest.TestCase):
    """Spec S4 item 5's cross-tier exclusion truth table."""

    def _claim(self, tier, tag_key=None, table=None, module=None):
        return {"tier": tier, "tag_key": tag_key, "canonical_table": table, "canonical_module": module}

    def test_t1_vs_t1_same_tag_conflicts(self):
        existing = [self._claim("T1", tag_key="JPEG:EXIF:ISO")]
        new = self._claim("T1", tag_key="JPEG:EXIF:ISO")
        self.assertTrue(claim_conflicts(existing, new))

    def test_t1_vs_t1_different_tag_same_table_does_not_conflict(self):
        existing = [self._claim("T1", tag_key="JPEG:EXIF:ISO", table="Canon::X")]
        new = self._claim("T1", tag_key="JPEG:EXIF:Other", table="Canon::X")
        self.assertFalse(claim_conflicts(existing, new))

    def test_t1_vs_t3_same_table_conflicts(self):
        existing = [self._claim("T3", table="Canon::X", module="Canon")]
        new = self._claim("T1", tag_key="JPEG:EXIF:ISO", table="Canon::X")
        self.assertTrue(claim_conflicts(existing, new))

    def test_t3_vs_t1_same_table_conflicts(self):
        existing = [self._claim("T1", tag_key="JPEG:EXIF:ISO", table="Canon::X")]
        new = self._claim("T3", table="Canon::X", module="Canon")
        self.assertTrue(claim_conflicts(existing, new))

    def test_t3_vs_t3_same_table_conflicts(self):
        existing = [self._claim("T3", table="Canon::X", module="Canon")]
        new = self._claim("T3", table="Canon::X", module="Canon")
        self.assertTrue(claim_conflicts(existing, new))

    def test_t1_vs_t3_different_tables_does_not_conflict(self):
        existing = [self._claim("T3", table="Canon::Y", module="Canon")]
        new = self._claim("T1", tag_key="JPEG:EXIF:ISO", table="Canon::X")
        self.assertFalse(claim_conflicts(existing, new))

    def test_t4_vs_t1_same_module_conflicts(self):
        existing = [self._claim("T4", module="FLIR")]
        new = self._claim("T1", tag_key="JPEG:FLIR:Foo", module="FLIR")
        self.assertTrue(claim_conflicts(existing, new))

    def test_t4_vs_t3_same_module_conflicts(self):
        existing = [self._claim("T4", module="FLIR")]
        new = self._claim("T3", table="FLIR::Records", module="FLIR")
        self.assertTrue(claim_conflicts(existing, new))

    def test_t4_vs_t4_same_module_conflicts(self):
        existing = [self._claim("T4", module="FLIR")]
        new = self._claim("T4", module="FLIR")
        self.assertTrue(claim_conflicts(existing, new))

    def test_t4_vs_anything_different_module_does_not_conflict(self):
        existing = [self._claim("T4", module="FLIR")]
        new = self._claim("T1", tag_key="JPEG:Canon:Foo", module="Canon")
        self.assertFalse(claim_conflicts(existing, new))

    def test_no_existing_claims_never_conflicts(self):
        self.assertFalse(claim_conflicts([], self._claim("T3", table="Canon::X", module="Canon")))

    def test_default_tier_is_t1(self):
        existing = [{"tag_key": "JPEG:EXIF:ISO", "canonical_table": None, "canonical_module": None}]
        new = {"tag_key": "JPEG:EXIF:ISO", "canonical_table": None, "canonical_module": None}
        self.assertTrue(claim_conflicts(existing, new))


class GatherLiveClaimsTests(unittest.TestCase):
    def test_stale_claim_is_excluded(self):
        state = {
            "JPEG:A:B": {"claimed_by": "w1", "claimed_at": 0, "tier": "T1"},
        }
        claims = gather_live_claims(state, time_fn=lambda: 10000, claim_stale_seconds=7200)
        self.assertEqual(claims, [])

    def test_live_claim_is_included_with_shape(self):
        state = {
            "JPEG:A:B": {
                "claimed_by": "w1", "claimed_at": 100, "tier": "T3",
                "canonical_table": "Canon::X", "canonical_module": "Canon",
            },
        }
        claims = gather_live_claims(state, time_fn=lambda: 200, claim_stale_seconds=7200)
        self.assertEqual(len(claims), 1)
        self.assertEqual(claims[0]["tier"], "T3")
        self.assertIsNone(claims[0]["tag_key"])  # T3 claims aren't tag_key-shaped
        self.assertEqual(claims[0]["canonical_table"], "Canon::X")

    def test_unclaimed_entry_is_excluded(self):
        state = {"JPEG:A:B": {"fails": 1, "blacklisted": False}}
        self.assertEqual(gather_live_claims(state, time_fn=lambda: 200), [])

    def test_exclude_key_omits_that_entry(self):
        state = {"JPEG:A:B": {"claimed_by": "w1", "claimed_at": 100}}
        claims = gather_live_claims(state, time_fn=lambda: 200, exclude_key="JPEG:A:B")
        self.assertEqual(claims, [])

    def test_default_tier_is_t1_and_tag_key_shaped(self):
        state = {"JPEG:A:B": {"claimed_by": "w1", "claimed_at": 100}}
        claims = gather_live_claims(state, time_fn=lambda: 200)
        self.assertEqual(claims[0]["tier"], "T1")
        self.assertEqual(claims[0]["tag_key"], "JPEG:A:B")


class ClaimTableAndFoundationJobTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.state_path = Path(self.tmpdir.name) / "state.json"

    def test_claims_table_job_when_uncontended(self):
        ok = claim_table_job(self.state_path, "Canon::CameraSettings", "Canon", "w1")
        self.assertTrue(ok)
        state = load_tag_state(self.state_path)
        key = table_job_claim_key("Canon::CameraSettings")
        self.assertEqual(state[key]["claimed_by"], "w1")
        self.assertEqual(state[key]["tier"], "T3")

    def test_renewing_own_table_job_claim_succeeds(self):
        claim_table_job(self.state_path, "Canon::CameraSettings", "Canon", "w1")
        ok = claim_table_job(self.state_path, "Canon::CameraSettings", "Canon", "w1")
        self.assertTrue(ok)

    def test_refuses_table_job_when_t1_claim_live_on_same_table(self):
        key = table_job_claim_key("dummy")
        state = {
            "JPEG:Canon:Foo": {
                "claimed_by": "w2", "claimed_at": time.time(), "tier": "T1",
                "canonical_table": "Canon::CameraSettings", "canonical_module": "Canon",
            },
        }
        save_tag_state(self.state_path, state)
        ok = claim_table_job(self.state_path, "Canon::CameraSettings", "Canon", "w1")
        self.assertFalse(ok)

    def test_release_table_job_claim_clears_claimed_by(self):
        claim_table_job(self.state_path, "Canon::CameraSettings", "Canon", "w1")
        release_table_job_claim(self.state_path, "Canon::CameraSettings")
        state = load_tag_state(self.state_path)
        key = table_job_claim_key("Canon::CameraSettings")
        self.assertNotIn("claimed_by", state[key])

    def test_claims_foundation_job_when_uncontended(self):
        job = {"name": "flir-fff", "target_module": "FLIR"}
        ok = claim_foundation_job(self.state_path, job, "w1")
        self.assertTrue(ok)
        state = load_tag_state(self.state_path)
        key = foundation_job_claim_key("FLIR")
        self.assertEqual(state[key]["tier"], "T4")

    def test_refuses_foundation_job_when_t3_claim_live_on_same_module(self):
        claim_table_job(self.state_path, "FLIR::Records", "FLIR", "w2")
        job = {"name": "flir-fff", "target_module": "FLIR"}
        ok = claim_foundation_job(self.state_path, job, "w1")
        self.assertFalse(ok)

    def test_release_foundation_job_claim_clears_claimed_by(self):
        job = {"name": "flir-fff", "target_module": "FLIR"}
        claim_foundation_job(self.state_path, job, "w1")
        release_foundation_job_claim(self.state_path, job)
        state = load_tag_state(self.state_path)
        key = foundation_job_claim_key("FLIR")
        self.assertNotIn("claimed_by", state[key])


class RunTagLoopCrossTierExclusionTests(unittest.TestCase):
    """Spec S4 item 5, wired into run_tag_loop's own claim() closure: a
    T1 tag claim on a table with a live T3 claim must be excluded."""

    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)
        self.state_path = str(Path(self.tmpdir.name) / "state.json")

    def test_t1_candidate_excluded_when_its_table_has_a_live_t3_claim(self):
        # Pre-seed a live T3 table-port claim on Canon::CameraSettings.
        save_tag_state(self.state_path, {
            table_job_claim_key("Canon::CameraSettings"): {
                "tier": "T3", "claimed_by": "merger-1", "claimed_at": time.time(),
                "canonical_table": "Canon::CameraSettings", "canonical_module": "Canon",
            },
        })
        attribution = {
            "tags": {
                "JPEG:MakerNotes:Foo": {"module": "Canon", "table": "CameraSettings"},
            },
        }
        gaps = [{
            "format": "JPEG",
            "missing_tags": [{"family": "MakerNotes", "name": "Foo", "value": "1", "source_file": None}],
            "value_differences": [], "gap_count": 1, "parser_files": [],
        }]

        result = run_tag_loop(
            {}, find_gaps_fn=lambda: gaps, fix_gap_fn=lambda *a: self.fail("must not attempt an excluded tag"),
            state_path=self.state_path, max_rounds=1, worker_id="w1", attribution=attribution,
        )
        # Nothing else claimable this round -> "wait", not an attempt.
        self.assertEqual(result["fixed"], [])
        self.assertEqual(result["failed"], [])

    def test_t1_candidate_not_excluded_without_attribution(self):
        """Backward compatibility: attribution=None (the default) must
        never newly exclude anything -- the whole feature is advisory
        and additive."""
        save_tag_state(self.state_path, {
            table_job_claim_key("Canon::CameraSettings"): {
                "tier": "T3", "claimed_by": "merger-1", "claimed_at": time.time(),
                "canonical_table": "Canon::CameraSettings", "canonical_module": "Canon",
            },
        })
        gaps = [{
            "format": "JPEG",
            "missing_tags": [{"family": "MakerNotes", "name": "Foo", "value": "1", "source_file": None}],
            "value_differences": [], "gap_count": 1, "parser_files": [],
        }]

        result = run_tag_loop(
            {}, find_gaps_fn=lambda: gaps,
            fix_gap_fn=lambda tag_gap, cfg, prev: {"status": "fixed", "gaps_closed": 1},
            state_path=self.state_path, max_rounds=1, worker_id="w1",
        )
        self.assertEqual(len(result["fixed"]), 1)

    def test_claim_stamps_tier_and_canonical_fields(self):
        attribution = {
            "tags": {
                "JPEG:MakerNotes:Foo": {"module": "Canon", "table": "CameraSettings"},
            },
        }
        gaps = [{
            "format": "JPEG",
            "missing_tags": [{"family": "MakerNotes", "name": "Foo", "value": "1", "source_file": None}],
            "value_differences": [], "gap_count": 1, "parser_files": [],
        }]
        captured = {}

        def fix_gap_fn(tag_gap, cfg, prev):
            captured["state"] = load_tag_state(self.state_path)
            return {"status": "fixed", "gaps_closed": 1}

        run_tag_loop(
            {}, find_gaps_fn=lambda: gaps, fix_gap_fn=fix_gap_fn,
            state_path=self.state_path, max_rounds=1, worker_id="w1", attribution=attribution,
        )
        entry = captured["state"]["JPEG:MakerNotes:Foo"]
        self.assertEqual(entry["tier"], "T1")
        self.assertEqual(entry["canonical_module"], "Canon")
        self.assertEqual(entry["canonical_table"], "Canon::CameraSettings")


def make_foundation_job(**overrides):
    job = {
        "name": "flir-fff-record-parser",
        "description": "Port the FLIR FFF record walker.",
        "target_formats": ["JPEG"],
        "target_module": "FLIR",
        "estimated_gaps": 90,
        "status": "pending",
    }
    job.update(overrides)
    return job


class BuildFoundationJobPromptTests(unittest.TestCase):
    def test_includes_job_identity_and_description(self):
        job = make_foundation_job()
        prompt = build_foundation_job_prompt(job)
        self.assertIn("flir-fff-record-parser", prompt)
        self.assertIn("FLIR", prompt)
        self.assertIn("Port the FLIR FFF record walker.", prompt)
        self.assertIn("T4 FOUNDATION-UNLOCK", prompt)


class AttemptFoundationJobTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmpdir.cleanup)

    def _fake_attempt_build(self, messages, **kwargs):
        messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
        return True, None, "--- a/x\n+++ b/x\n", messages

    def test_lands_a_commit_on_the_happy_path(self):
        job = make_foundation_job()
        commit_calls = []
        result = attempt_foundation_job(
            job, Path("/fake/repo"), CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            review_fn=lambda g, diff, config, **kw: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: commit_calls.append(msg),
            git_rev_parse_fn=lambda root: "abc123",
            log_fn=lambda *a: None,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(result["commit_sha"], "abc123")
        self.assertEqual(len(commit_calls), 1)

    def test_build_failure_retries_then_fails(self):
        job = make_foundation_job()

        def always_fails(messages, **kwargs):
            return False, "compile error", None, messages

        result = attempt_foundation_job(
            job, Path("/fake/repo"), CONFIG,
            attempt_build_fn=always_fails,
            critique_fn=lambda *a, **kw: "try a different approach",
            git_checkout_clean_fn=lambda root: None,
            log_fn=lambda *a: None,
            table_job_config={"max_prompt_tokens": 16384, "max_repair_rounds": 2},
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(len(result["rounds"]), 2)

    def test_marks_held_by_foundation_on_matching_tags(self):
        state_path = Path(self.tmpdir.name) / "state.json"
        save_tag_state(state_path, {
            "JPEG:FLIR:Temp": {"fails": 0, "blacklisted": False, "canonical_module": "FLIR"},
            "JPEG:Canon:Other": {"fails": 0, "blacklisted": False, "canonical_module": "Canon"},
        })
        job = make_foundation_job()
        result = attempt_foundation_job(
            job, Path("/fake/repo"), CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            review_fn=lambda g, diff, config, **kw: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: None,
            git_rev_parse_fn=lambda root: "deadbeef",
            log_fn=lambda *a: None,
            state_path=state_path,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(result["held_tags"], ["JPEG:FLIR:Temp"])
        state = load_tag_state(state_path)
        self.assertEqual(state["JPEG:FLIR:Temp"]["held_by_foundation"], {"job": job["name"], "sha": "deadbeef"})
        self.assertNotIn("held_by_foundation", state["JPEG:Canon:Other"])

    def test_review_rejection_retries_then_fails(self):
        job = make_foundation_job()
        result = attempt_foundation_job(
            job, Path("/fake/repo"), CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            review_fn=lambda g, diff, config, **kw: (False, "hardcodes a sample value"),
            critique_fn=lambda *a, **kw: "be more general",
            git_checkout_clean_fn=lambda root: None,
            log_fn=lambda *a: None,
            table_job_config={"max_prompt_tokens": 16384, "max_repair_rounds": 1},
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("rejected by review", result["reason"])


class ResolveFoundationJobTagKeysTests(unittest.TestCase):
    def test_matches_by_stamped_canonical_module(self):
        job = make_foundation_job(target_module="FLIR")
        state = {
            "JPEG:FLIR:Temp": {"canonical_module": "FLIR"},
            "JPEG:Canon:Other": {"canonical_module": "Canon"},
        }
        self.assertEqual(resolve_foundation_job_tag_keys(job, state), ["JPEG:FLIR:Temp"])

    def test_matches_via_attribution_when_state_has_no_canonical_module(self):
        job = make_foundation_job(target_module="FLIR")
        state = {"JPEG:FLIR:Temp": {"fails": 0}}
        attribution = {"tags": {"JPEG:FLIR:Temp": {"module": "FLIR", "table": ""}}}
        self.assertEqual(resolve_foundation_job_tag_keys(job, state, attribution), ["JPEG:FLIR:Temp"])

    def test_never_matches_synthetic_job_claim_keys(self):
        job = make_foundation_job(target_module="FLIR")
        state = {
            table_job_claim_key("FLIR::Records"): {"canonical_module": "FLIR"},
            foundation_job_claim_key("FLIR"): {"canonical_module": "FLIR"},
        }
        self.assertEqual(resolve_foundation_job_tag_keys(job, state), [])

    def test_no_target_module_returns_empty(self):
        job = make_foundation_job(target_module=None)
        self.assertEqual(resolve_foundation_job_tag_keys(job, {"x": {"canonical_module": None}}), [])


class MarkHeldByFoundationTests(unittest.TestCase):
    def test_stamps_matching_entries_only(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            state_path = Path(tmpdir) / "state.json"
            save_tag_state(state_path, {
                "JPEG:FLIR:Temp": {"canonical_module": "FLIR"},
                "JPEG:Canon:Other": {"canonical_module": "Canon"},
            })
            job = make_foundation_job(target_module="FLIR")
            stamped = mark_held_by_foundation(state_path, job, "sha123")
            self.assertEqual(stamped, ["JPEG:FLIR:Temp"])
            state = load_tag_state(state_path)
            self.assertEqual(state["JPEG:FLIR:Temp"]["held_by_foundation"], {"job": job["name"], "sha": "sha123"})

    def test_skips_tag_keys_not_present_in_state(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            state_path = Path(tmpdir) / "state.json"
            save_tag_state(state_path, {})
            job = make_foundation_job(target_module="FLIR")
            stamped = mark_held_by_foundation(state_path, job, "sha123")
            self.assertEqual(stamped, [])


class TablePortPseudoGapTests(unittest.TestCase):
    """_table_port_pseudo_gap must actually populate a non-empty
    missing_tags set from table_members -- without it,
    find_implemented_sibling's `families` search set is empty by
    construction and its whole search loop never executes, regardless of
    what the target registry file contains (see find_implemented_sibling/
    build_neighbor_precedent_block)."""

    def test_no_table_members_keeps_missing_tags_empty(self):
        gap = _table_port_pseudo_gap("Canon::CameraSettings", "Canon", ".")
        self.assertEqual(gap["missing_tags"], [])
        self.assertEqual(gap["value_differences"], [])

    def test_table_members_populate_missing_tags_family_and_name(self):
        gap = _table_port_pseudo_gap(
            "Canon::CameraSettings", "Canon", ".",
            table_members=["Canon:MacroMode", "Canon:FlashMode"],
        )
        self.assertEqual(
            gap["missing_tags"],
            [{"family": "Canon", "name": "MacroMode"}, {"family": "Canon", "name": "FlashMode"}],
        )

    def test_malformed_member_without_colon_is_skipped(self):
        gap = _table_port_pseudo_gap("Canon::CameraSettings", "Canon", ".", table_members=["NoColonHere"])
        self.assertEqual(gap["missing_tags"], [])

    def test_sibling_search_actually_runs_given_table_members(self):
        # End-to-end: with table_members threaded through, a sibling
        # literal actually already present in the registry file (for a
        # DIFFERENT tag of the same family) is found -- proving the
        # search space is no longer unconditionally empty.
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            registries_dir = repo_root / "src" / "parsers" / "tiff" / "makernotes" / "registries"
            registries_dir.mkdir(parents=True)
            (registries_dir / "canon.rs").write_text('metadata.insert("Canon:ColorMode".to_string(), v);')
            gap = _table_port_pseudo_gap(
                "Canon::CameraSettings", "Canon", repo_root,
                table_members=["Canon:MacroMode"],
            )
            self.assertEqual(find_implemented_sibling(gap, repo_root), "Canon:ColorMode")

    def test_without_table_members_sibling_search_never_runs_even_with_a_match_present(self):
        # The regression this whole test class guards against: before
        # table_members was threaded through, missing_tags was always
        # [], so `families` was always empty and this returned None
        # unconditionally -- even with a matching sibling literal sitting
        # right there in the registry file.
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            registries_dir = repo_root / "src" / "parsers" / "tiff" / "makernotes" / "registries"
            registries_dir.mkdir(parents=True)
            (registries_dir / "canon.rs").write_text('metadata.insert("Canon:ColorMode".to_string(), v);')
            gap = _table_port_pseudo_gap("Canon::CameraSettings", "Canon", repo_root)
            self.assertIsNone(find_implemented_sibling(gap, repo_root))


class BuildTablePortPromptTests(unittest.TestCase):
    def test_includes_three_clause_language_and_table_identity(self):
        prompt = build_table_port_prompt("Canon::CameraSettings", "Canon", "PERL SOURCE HERE", "SKELETON HERE")
        self.assertIn("Canon::CameraSettings", prompt)
        self.assertIn("T3 TABLE-PORT", prompt)
        self.assertIn("PERL SOURCE HERE", prompt)
        self.assertIn("SKELETON HERE", prompt)
        self.assertIn("80%", prompt)

    def test_handles_missing_perl_source_gracefully(self):
        prompt = build_table_port_prompt("Canon::CameraSettings", "Canon", None, None)
        self.assertIn("unavailable", prompt)


class AttemptTablePortTests(unittest.TestCase):
    def _fake_attempt_build(self, messages, **kwargs):
        messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
        return True, None, "--- a/x\n+++ b/x\n", messages

    def test_lands_a_commit_when_gate_passes(self):
        members = ["Canon:A", "Canon:B", "Canon:C", "Canon:D", "Canon:E"]
        pre_report = {
            "missing_tags": [{"family": "Canon", "name": n.split(":")[1]} for n in members],
            "value_differences": [],
        }
        post_report = {"missing_tags": [], "value_differences": []}
        commit_calls = []

        result = attempt_table_port(
            "Canon::CameraSettings", "Canon", Path("/fake/repo"), CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            review_fn=lambda g, diff, config, **kw: (True, ""),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: commit_calls.append((msg, kw.get("trailers"))),
            log_fn=lambda *a: None,
            table_members=members, pre_report=pre_report,
            recheck_fn=lambda fmt: post_report,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(commit_calls), 1)
        _, trailers = commit_calls[0]
        self.assertEqual(trailers["Table"], "Canon::CameraSettings")

    def test_gate_failure_retries_with_must_remove_guidance_then_fails(self):
        members = ["Canon:A", "Canon:B"]
        pre_report = {"missing_tags": [{"family": "Canon", "name": "A"}, {"family": "Canon", "name": "B"}],
                     "value_differences": []}
        post_report_wrong = {"missing_tags": [], "value_differences": [{"tag_key": "Canon:A"}]}
        critiques = []

        result = attempt_table_port(
            "Canon::CameraSettings", "Canon", Path("/fake/repo"), CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            critique_fn=lambda gap, diff, kind, detail, cfg, **kw: (critiques.append(detail), "fix it")[1],
            git_checkout_clean_fn=lambda root: None,
            log_fn=lambda *a: None,
            table_members=members, pre_report=pre_report,
            recheck_fn=lambda fmt: post_report_wrong,
            table_job_config={"max_prompt_tokens": 16384, "max_repair_rounds": 1},
        )
        self.assertEqual(result["status"], "failed")
        self.assertTrue(any("Canon:A" in c for c in critiques))

    def test_review_rejection_after_gate_pass_still_fails_without_commit(self):
        members = ["Canon:A"]
        pre_report = {"missing_tags": [{"family": "Canon", "name": "A"}], "value_differences": []}
        post_report = {"missing_tags": [], "value_differences": []}

        result = attempt_table_port(
            "Canon::CameraSettings", "Canon", Path("/fake/repo"), CONFIG,
            attempt_build_fn=self._fake_attempt_build,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            review_fn=lambda g, diff, config, **kw: (False, "hardcodes a value"),
            critique_fn=lambda *a, **kw: None,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root, **kw: self.fail("should not commit"),
            log_fn=lambda *a: None,
            table_members=members, pre_report=pre_report,
            recheck_fn=lambda fmt: post_report,
            table_job_config={"max_prompt_tokens": 16384, "max_repair_rounds": 1},
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("rejected by review", result["reason"])


class NormalizeTableJobConfigTests(unittest.TestCase):
    def test_defaults_when_section_absent(self):
        cfg = normalize_table_job_config({})
        self.assertEqual(cfg["max_prompt_tokens"], DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS)
        self.assertEqual(cfg["max_repair_rounds"], DEFAULT_TABLE_JOB_MAX_REPAIR_ROUNDS)

    def test_reads_explicit_section(self):
        cfg = normalize_table_job_config({"table_job": {"max_prompt_tokens": 8192, "max_repair_rounds": 3}})
        self.assertEqual(cfg["max_prompt_tokens"], 8192)
        self.assertEqual(cfg["max_repair_rounds"], 3)


if __name__ == "__main__":
    unittest.main()
