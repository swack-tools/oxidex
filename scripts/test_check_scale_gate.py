import json
import tempfile
import unittest
from pathlib import Path

from check_scale_gate import (
    disk_headroom_gate,
    format_report,
    governor_gate,
    read_configured_governor_rate,
    read_governor_consecutive_limited,
    run_all_gates,
    t3_calls_per_tag_gate,
)


class ReadConfiguredGovernorRateTests(unittest.TestCase):
    def test_missing_file_returns_none(self):
        self.assertIsNone(read_configured_governor_rate("/nonexistent/config.toml"))

    def test_missing_worker_table_returns_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "config.toml"
            path.write_text('[reviewer]\nbase_url = "u"\n')
            self.assertIsNone(read_configured_governor_rate(path))

    def test_reads_the_configured_rate(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "config.toml"
            path.write_text('[worker]\ngovernor_calls_per_minute = 60\n')
            self.assertEqual(read_configured_governor_rate(path), 60)

    def test_malformed_toml_returns_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "config.toml"
            path.write_text('[worker\nnot valid toml')
            self.assertIsNone(read_configured_governor_rate(path))


class ReadGovernorConsecutiveLimitedTests(unittest.TestCase):
    def test_missing_file_returns_none(self):
        self.assertIsNone(read_governor_consecutive_limited("/nonexistent/rate-governor.json"))

    def test_reads_the_counter(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "rate-governor.json"
            path.write_text(json.dumps({"consecutive_limited": 3}))
            self.assertEqual(read_governor_consecutive_limited(path), 3)

    def test_corrupt_file_returns_none(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "rate-governor.json"
            path.write_text("{not json")
            self.assertIsNone(read_governor_consecutive_limited(path))


class GovernorGateTests(unittest.TestCase):
    def test_passes_when_configured_rate_meets_the_required_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = Path(tmpdir) / "config.toml"
            config_path.write_text('[worker]\ngovernor_calls_per_minute = 60\n')
            gate = governor_gate(config_path, Path(tmpdir) / "nonexistent.json")
            self.assertTrue(gate["passed"])
            self.assertEqual(gate["measured"], 60)

    def test_fails_when_below_the_required_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = Path(tmpdir) / "config.toml"
            config_path.write_text('[worker]\ngovernor_calls_per_minute = 30\n')
            gate = governor_gate(config_path, Path(tmpdir) / "nonexistent.json")
            self.assertFalse(gate["passed"])

    def test_fails_closed_when_config_is_unreadable(self):
        gate = governor_gate("/nonexistent/config.toml", "/nonexistent/rate-governor.json")
        self.assertFalse(gate["passed"])
        self.assertIsNone(gate["measured"])

    def test_detail_mentions_consecutive_limited_when_present(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = Path(tmpdir) / "config.toml"
            config_path.write_text('[worker]\ngovernor_calls_per_minute = 60\n')
            governor_path = Path(tmpdir) / "rate-governor.json"
            governor_path.write_text(json.dumps({"consecutive_limited": 5}))
            gate = governor_gate(config_path, governor_path)
            self.assertIn("consecutive_limited=5", gate["detail"])


class T3CallsPerTagGateTests(unittest.TestCase):
    def _write_logs(self, tmpdir, manifest_lines, found_lines):
        manifest_path = Path(tmpdir) / "manifest.log"
        manifest_path.write_text(manifest_lines)
        found_path = Path(tmpdir) / "tags-found.log"
        found_path.write_text(found_lines)
        return manifest_path, found_path

    def test_no_t3_data_fails_with_no_data_detail(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            manifest_path, found_path = self._write_logs(tmpdir, "", "")
            gate = t3_calls_per_tag_gate(manifest_path, found_path)
            self.assertFalse(gate["passed"])
            self.assertIn("no T3", gate["detail"])
            self.assertIsNone(gate["measured"])

    def test_passes_when_under_the_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            manifest_lines = (
                "2026-07-24T10:00:00 phase=fixer worker=1 tier=T3 model=m prompt_chars=1 "
                "elapsed=1.0s reply_chars=1 OK\n"
                "2026-07-24T10:01:00 phase=reviewer worker=1 tier=T3 model=m prompt_chars=1 "
                "elapsed=1.0s reply_chars=1 OK\n"
            )
            found_lines = "2026-07-24T10:02:00 worker=1 tag=Canon:CameraSettings gaps_closed=5 tier=T3\n"
            manifest_path, found_path = self._write_logs(tmpdir, manifest_lines, found_lines)
            gate = t3_calls_per_tag_gate(manifest_path, found_path, required=10)
            self.assertTrue(gate["passed"])
            self.assertEqual(gate["measured"], 2.0)

    def test_fails_when_at_or_over_the_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            lines = "".join(
                f"2026-07-24T10:00:{i:02d} phase=fixer worker=1 tier=T3 model=m prompt_chars=1 "
                f"elapsed=1.0s reply_chars=1 OK\n"
                for i in range(10)
            )
            found_lines = "2026-07-24T10:02:00 worker=1 tag=Canon:CameraSettings gaps_closed=5 tier=T3\n"
            manifest_path, found_path = self._write_logs(tmpdir, lines, found_lines)
            gate = t3_calls_per_tag_gate(manifest_path, found_path, required=10)
            self.assertFalse(gate["passed"])
            self.assertEqual(gate["measured"], 10.0)


class DiskHeadroomGateTests(unittest.TestCase):
    def test_passes_with_a_low_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            gate = disk_headroom_gate(tmpdir, required_gb=0.001)
            self.assertTrue(gate["passed"])
            self.assertIsInstance(gate["measured"], float)

    def test_fails_with_an_absurdly_high_threshold(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            gate = disk_headroom_gate(tmpdir, required_gb=10 ** 9)
            self.assertFalse(gate["passed"])

    def test_unreadable_path_fails_closed(self):
        gate = disk_headroom_gate("/this/path/does/not/exist/at/all")
        self.assertFalse(gate["passed"])
        self.assertIsNone(gate["measured"])


class RunAllGatesTests(unittest.TestCase):
    def test_overall_passed_is_false_when_any_gate_fails(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = Path(tmpdir) / "config.toml"
            config_path.write_text('[worker]\ngovernor_calls_per_minute = 30\n')  # below threshold
            result = run_all_gates(
                config_path=config_path,
                governor_path=Path(tmpdir) / "rate-governor.json",
                manifest_path=Path(tmpdir) / "manifest.log",
                tags_found_log_path=Path(tmpdir) / "tags-found.log",
                disk_path=tmpdir,
            )
            self.assertFalse(result["overall_passed"])
            self.assertEqual(len(result["gates"]), 3)

    def test_format_report_includes_every_gate_and_the_verdict(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = Path(tmpdir) / "config.toml"
            config_path.write_text('[worker]\ngovernor_calls_per_minute = 60\n')
            result = run_all_gates(
                config_path=config_path,
                governor_path=Path(tmpdir) / "rate-governor.json",
                manifest_path=Path(tmpdir) / "manifest.log",
                tags_found_log_path=Path(tmpdir) / "tags-found.log",
                disk_path=tmpdir,
            )
            report = format_report(result)
            self.assertIn("governor_calls_per_minute", report)
            self.assertIn("t3_calls_per_landed_tag", report)
            self.assertIn("disk_headroom_gb", report)
            self.assertIn("OVERALL:", report)


if __name__ == "__main__":
    unittest.main()
