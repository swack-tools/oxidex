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
        self.assertEqual(s["deepseek-v4-pro"]["ok"], 2)
        self.assertEqual(s["deepseek-v4-pro"]["retry"], 0)
        self.assertEqual(s["glm-5.2"]["ok"], 1)
        self.assertEqual(s["glm-5.2"]["retry"], 1)

    def test_success_rate_and_latency(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = score_models(self._write(tmp))
        self.assertAlmostEqual(s["deepseek-v4-pro"]["success_rate"], 1.0)
        self.assertAlmostEqual(s["glm-5.2"]["success_rate"], 0.5)
        self.assertAlmostEqual(s["deepseek-v4-pro"]["p50_latency"], 15.0)

    def test_since_filter_excludes_older_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = score_models(self._write(tmp), since_iso="2026-07-25T06:43:00")
        self.assertNotIn("deepseek-v4-pro", s)

    def test_fix_attribution_from_commit_subjects(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = self._write(tmp)
            def fake_git(args, cwd):
                return "fix(elf): wire 2 missing tags (via glm-5.2/deepseek-v4-pro)\nchore: noise\n"
            s = score_models(p, repo="/unused", git_run=fake_git)
        self.assertEqual(s["glm-5.2"]["fixes"], 1)
        self.assertEqual(s["deepseek-v4-pro"]["fixes"], 1)

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
