"""Hermetic tests for model_rotator.py -- no network, no live config.

The daemon edits the config 20+ workers read every round, so the tests
that matter most are the refusals: never install an unparseable config,
never leave a pool empty, never clobber the tuning comments.
"""
import json
import tempfile
import unittest
from pathlib import Path

from model_rotator import (
    MANAGED,
    config_is_safe,
    install_config,
    probe_model,
    rank_candidates,
    read_pool,
    rewrite_pool,
    score_models,
)

CONFIG = '''[worker]
base_url = "https://api.example.com/v1"
api_key = "k"
# EXPERIMENT 11 -- a tuning comment that must survive a rewrite
max_cluster_tags = 12

[[worker.models]]
name = "deepseek-v4-pro"
phase = "explore"

[[worker.models]]
name = "deepseek-v4-pro"
phase = "patch"

[reviewer]
base_url = "https://api.example.com/v1"
api_key = "k"

[[reviewer.models]]
name = "deepseek-v4-pro"

[table_job]
max_repair_rounds = 8

[[table_job.models]]
name = "deepseek-v4-pro"
phase = "table"
'''


class ReadPoolTests(unittest.TestCase):
    def test_finds_every_worker_model_block(self):
        self.assertEqual([n for _, n in read_pool(CONFIG, "worker")],
                         ["deepseek-v4-pro", "deepseek-v4-pro"])

    def test_finds_single_entry_sections(self):
        self.assertEqual([n for _, n in read_pool(CONFIG, "reviewer")], ["deepseek-v4-pro"])
        self.assertEqual([n for _, n in read_pool(CONFIG, "table_job")], ["deepseek-v4-pro"])

    def test_unknown_section_is_empty_not_an_error(self):
        self.assertEqual(read_pool(CONFIG, "nope"), [])


class RewritePoolTests(unittest.TestCase):
    def test_replaces_names_across_both_worker_blocks(self):
        out = rewrite_pool(CONFIG, "worker", ["kimi-k2.7-code", "glm-5.2"])
        self.assertEqual([n for _, n in read_pool(out, "worker")],
                         ["kimi-k2.7-code", "glm-5.2"])

    def test_preserves_phase_lines(self):
        out = rewrite_pool(CONFIG, "worker", ["glm-5.2"])
        self.assertIn('phase = "explore"', out)
        self.assertIn('phase = "patch"', out)

    def test_preserves_tuning_comments(self):
        # A tomllib round-trip would delete every comment in the file;
        # the whole point of line-targeted rewriting is that it does not.
        out = rewrite_pool(CONFIG, "worker", ["glm-5.2"])
        self.assertIn("EXPERIMENT 11 -- a tuning comment that must survive", out)
        self.assertIn("max_cluster_tags = 12", out)

    def test_fewer_names_than_slots_cycles_rather_than_emptying(self):
        out = rewrite_pool(CONFIG, "worker", ["glm-5.2"])
        self.assertEqual([n for _, n in read_pool(out, "worker")], ["glm-5.2", "glm-5.2"])

    def test_empty_name_list_is_a_no_op(self):
        self.assertEqual(rewrite_pool(CONFIG, "worker", []), CONFIG)

    def test_other_sections_are_untouched(self):
        out = rewrite_pool(CONFIG, "worker", ["glm-5.2"])
        self.assertEqual([n for _, n in read_pool(out, "reviewer")], ["deepseek-v4-pro"])


class ConfigSafetyTests(unittest.TestCase):
    def test_valid_config_is_safe(self):
        ok, why = config_is_safe(CONFIG)
        self.assertTrue(ok, why)

    def test_unparseable_config_is_refused(self):
        ok, why = config_is_safe("[[[ not toml")
        self.assertFalse(ok)
        self.assertIn("TOML parse failed", why)

    def test_empty_pool_is_refused(self):
        broken = CONFIG.replace('[[reviewer.models]]\nname = "deepseek-v4-pro"\n', "")
        ok, why = config_is_safe(broken)
        self.assertFalse(ok)
        self.assertIn("reviewer", why)

    def test_model_without_a_name_is_refused(self):
        broken = CONFIG.replace('[[reviewer.models]]\nname = "deepseek-v4-pro"',
                                '[[reviewer.models]]\nphase = "x"')
        ok, why = config_is_safe(broken)
        self.assertFalse(ok)

    def test_install_refuses_bad_config_and_leaves_the_original(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "config.toml"
            p.write_text(CONFIG)
            installed = install_config(p, "[[[ not toml", log=lambda m: None)
            self.assertFalse(installed)
            self.assertEqual(p.read_text(), CONFIG)  # untouched

    def test_install_writes_a_good_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "config.toml"
            p.write_text(CONFIG)
            new = rewrite_pool(CONFIG, "worker", ["glm-5.2"])
            self.assertTrue(install_config(p, new, log=lambda m: None))
            self.assertIn('name = "glm-5.2"', p.read_text())


class ScoreModelsTests(unittest.TestCase):
    MANIFEST = (
        "2026-07-25T06:41:00 phase=fixer worker=a model=deepseek-v4-pro elapsed=10.0s reply_chars=5 OK\n"
        "2026-07-25T06:42:00 phase=fixer worker=a model=deepseek-v4-pro elapsed=20.0s reply_chars=5 OK\n"
        "2026-07-25T06:43:00 phase=fixer worker=b model=glm-5.2 RETRY model call retry 1/10 after X\n"
        "2026-07-25T06:44:00 phase=fixer worker=b model=glm-5.2 elapsed=99.0s reply_chars=5 OK\n"
    )

    def _write(self, tmp):
        p = Path(tmp) / "manifest.log"
        p.write_text(self.MANIFEST)
        return p

    def test_counts_ok_and_retry_per_model(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = score_models(self._write(tmp))
        self.assertEqual(s["theclawbay-deepseek-v4-pro"]["ok"], 2)
        self.assertEqual(s["theclawbay-deepseek-v4-pro"]["retry"], 0)
        self.assertEqual(s["theclawbay-glm-5.2"]["ok"], 1)
        self.assertEqual(s["theclawbay-glm-5.2"]["retry"], 1)

    def test_success_rate_and_latency(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = score_models(self._write(tmp))
        self.assertAlmostEqual(s["theclawbay-deepseek-v4-pro"]["success_rate"], 1.0)
        self.assertAlmostEqual(s["theclawbay-glm-5.2"]["success_rate"], 0.5)
        self.assertAlmostEqual(s["theclawbay-deepseek-v4-pro"]["p50_latency"], 15.0)

    def test_since_filter_excludes_older_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = score_models(self._write(tmp), since_iso="2026-07-25T06:43:00")
        self.assertNotIn("theclawbay-deepseek-v4-pro", s)

    def test_fix_attribution_from_commit_subjects(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = self._write(tmp)
            def fake_git(args, cwd):
                return "fix(elf): wire 2 missing tags (via glm-5.2/deepseek-v4-pro)\nchore: noise\n"
            s = score_models(p, repo="/unused", git_run=fake_git)
        self.assertEqual(s["theclawbay-glm-5.2"]["fixes"], 1)
        self.assertEqual(s["theclawbay-deepseek-v4-pro"]["fixes"], 1)

    def test_missing_manifest_is_empty_not_a_crash(self):
        self.assertEqual(score_models("/nonexistent/manifest.log"), {})


class RankCandidatesTests(unittest.TestCase):
    def test_higher_success_rate_wins(self):
        scores = {"a": {"success_rate": 0.99, "p50_latency": 50, "fixes": 0},
                  "b": {"success_rate": 0.25, "p50_latency": 5, "fixes": 0}}
        self.assertEqual(rank_candidates(["b", "a"], scores)[0], "a")

    def test_fixes_break_a_success_rate_tie(self):
        scores = {"a": {"success_rate": 0.99, "p50_latency": 50, "fixes": 0},
                  "b": {"success_rate": 0.99, "p50_latency": 50, "fixes": 3}}
        self.assertEqual(rank_candidates(["a", "b"], scores)[0], "b")

    def test_lower_latency_breaks_remaining_ties(self):
        scores = {"a": {"success_rate": 0.99, "p50_latency": 200, "fixes": 0},
                  "b": {"success_rate": 0.99, "p50_latency": 20, "fixes": 0}}
        self.assertEqual(rank_candidates(["a", "b"], scores)[0], "b")

    def test_unscored_model_sorts_midpack_not_last(self):
        # A newly recovered model must actually get tried, not be starved
        # forever by incumbents that already have a record.
        scores = {"known_bad": {"success_rate": 0.30, "p50_latency": 10, "fixes": 0}}
        ranked = rank_candidates(["known_bad", "brand_new"], scores)
        self.assertEqual(ranked[0], "brand_new")

    def test_empty_healthy_list_is_empty(self):
        self.assertEqual(rank_candidates([], {}), [])

    def test_history_matches_across_casing(self):
        # manifest.log records "DeepSeek-V4-Pro"; the API's canonical id is
        # "deepseek-v4-pro". Without case folding, every model looks
        # unscored and the whole scoreboard is silently ignored.
        scores = {"Kimi-K2.6": {"success_rate": 0.999, "p50_latency": 30, "fixes": 0},
                  "gpt-5.6-sol": {"success_rate": 0.258, "p50_latency": 58, "fixes": 0}}
        ranked = rank_candidates(["gpt-5.6-sol", "kimi-k2.6"], scores)
        self.assertEqual(ranked[0], "kimi-k2.6")


class ProbeModelTests(unittest.TestCase):
    def test_non_json_or_unreachable_host_is_down_not_an_exception(self):
        up, detail = probe_model("http://127.0.0.1:9/v1", "k", "m", timeout=2)
        self.assertFalse(up)
        self.assertTrue(detail)


class ManagedSectionsTests(unittest.TestCase):
    def test_every_managed_section_exists_in_a_realistic_config(self):
        for section in MANAGED:
            self.assertTrue(read_pool(CONFIG, section), f"{section} pool not found")


if __name__ == "__main__":
    unittest.main()


class CandidatesForProviderTests(unittest.TestCase):
    def test_wafer_endpoint_gets_wafer_model_ids(self):
        # Probing clawbay's lowercase ids against wafer finds nothing
        # healthy, which would wedge the daemon into never rotating.
        from model_rotator import candidates_for
        c = candidates_for("https://pass.wafer.ai/v1")
        self.assertIn("Kimi-K2.6", c)
        self.assertNotIn("gpt-5.6", c)

    def test_clawbay_endpoint_gets_clawbay_model_ids(self):
        from model_rotator import candidates_for
        c = candidates_for("https://api.theclawbay.com/v1")
        self.assertIn("deepseek-v4-pro", c)
        self.assertIn("gpt-5.6", c)

    def test_config_override_wins_over_endpoint_defaults(self):
        from model_rotator import candidates_for
        self.assertEqual(
            candidates_for("https://pass.wafer.ai/v1",
                           {"rotator": {"candidates": ["only-this"]}}),
            ["only-this"])

    def test_unknown_endpoint_falls_back_rather_than_raising(self):
        from model_rotator import candidates_for
        self.assertTrue(candidates_for("https://brand-new-provider/v1"))


class ProviderSplitTests(unittest.TestCase):
    """The same model id on two providers must never share a bucket --
    that is what made a clawbay quota failure look like the model itself
    degrading (DeepSeek read 98.7% on wafer, 81.8% blended with clawbay)."""

    MANIFEST = (
        "2026-07-25T14:00:00 phase=fixer worker=a provider=wafer model=DeepSeek-V4-Pro elapsed=10.0s OK\n"
        "2026-07-25T14:01:00 phase=fixer worker=a provider=theclawbay model=deepseek-v4-pro RETRY "
        "model call retry 1/10 after <HTTPError 429: 'Too Many Requests'>\n"
        "2026-07-25T14:02:00 phase=fixer worker=a provider=theclawbay model=deepseek-v4-pro RETRY "
        "model call retry 2/10 after <HTTPError 503: 'Service Unavailable'>\n"
        "2026-07-25T14:03:00 phase=fixer worker=a provider=wafer model=DeepSeek-V4-Pro RETRY "
        "model call retry 1/10 after RuntimeError('model returned an empty reply')\n"
    )

    def _score(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "manifest.log"
            p.write_text(self.MANIFEST)
            return score_models(p)

    def test_same_model_splits_by_provider(self):
        s = self._score()
        self.assertIn("wafer-DeepSeek-V4-Pro", s)
        self.assertIn("theclawbay-deepseek-v4-pro", s)

    def test_429_and_500_are_separate_from_reply_level_retry(self):
        s = self._score()
        cb = s["theclawbay-deepseek-v4-pro"]
        self.assertEqual(cb["http_429"], 1)
        self.assertEqual(cb["http_500"], 1)
        self.assertEqual(cb["retry"], 0)   # neither is a reply-quality failure

    def test_empty_reply_counts_as_retry_not_http(self):
        s = self._score()
        w = s["wafer-DeepSeek-V4-Pro"]
        self.assertEqual(w["retry"], 1)
        self.assertEqual(w["http_429"], 0)
        self.assertEqual(w["http_500"], 0)

    def test_quota_failures_count_against_success_rate(self):
        # 1 OK out of 3 attempts on clawbay -> quota costs the same
        # wall-clock as any other failure, so it must not be excluded.
        s = self._score()
        self.assertAlmostEqual(s["theclawbay-deepseek-v4-pro"]["success_rate"], 0.0)
        self.assertAlmostEqual(s["wafer-DeepSeek-V4-Pro"]["success_rate"], 0.5)

    def test_legacy_lines_without_provider_are_inferred_by_casing(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "manifest.log"
            p.write_text(
                "2026-07-25T01:00:00 phase=fixer worker=a model=Kimi-K2.6 elapsed=5.0s OK\n"
                "2026-07-25T01:01:00 phase=fixer worker=a model=gpt-5.5 elapsed=5.0s OK\n")
            s = score_models(p)
        self.assertIn("wafer-Kimi-K2.6", s)
        self.assertIn("theclawbay-gpt-5.5", s)
