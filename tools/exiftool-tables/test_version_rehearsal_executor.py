"""Offline scheduler tests for the non-promoting version rehearsal executor."""
from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import version_rehearsal_executor as executor
import version_rehearsal_native_oracle as native
import test_version_rehearsal_native_oracle as fixture


def ready_probe(release: str) -> dict:
    payload = {"schema": native.SCHEMA, "kind": native.KIND, "identity": {"release": release},
               "cases": [{"name": "case", "state": "ready"}], "state": "ready"}
    return {**payload, "probe_sha256": native.catalog_stage.sha256_json(payload)}


class ExecutorTests(unittest.TestCase):
    def setUp(self):
        temporary, capture, catalog, plan, resolution, materialization, cache, sources, _ = fixture.make_state()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.capture, self.catalog, self.plan = capture, catalog, plan
        self.resolution, self.materialization = resolution, materialization
        self.cache, self.sources = cache, sources
        self.repository = self.root / "oxidex-repository"
        (self.repository / ".git").mkdir(parents=True)
        self.run_dir = self.root / "execution"
        self.releases = sorted({side["release"] for pair in plan["pairs"] for side in (pair["old"], pair["new"])})
        self.calls, self.checkouts, self.native_calls = [], [], []

    def config(self, *, write=True):
        commands = {stage: {"argv": [stage]} for stage in ("generate", "build", "read")}
        if write:
            commands["write"] = {"argv": ["write"]}
        return {"schema": executor.SCHEMA, "commands": commands,
                "perls": {release: sys.executable for release in self.releases},
                "native_cases": {release: [{"case": release}] for release in self.releases}}

    def initialize(self, config):
        return executor.initialize_run(self.run_dir, self.capture, self.catalog, self.plan,
                                       self.resolution, self.materialization, config)

    def checkout(self, repository, commit, destination, run):
        self.checkouts.append((repository, commit, destination))
        destination.mkdir(parents=True)
        return destination

    def command(self, argv, **kwargs):
        self.calls.append((argv, kwargs["cwd"], kwargs["env"]["CARGO_TARGET_DIR"]))
        stage = argv[0]
        env = kwargs["env"]
        report = Path(env["OXIDEX_REHEARSAL_REPORT"])
        body = {"schema": executor.SCHEMA, "kind": executor.RESULT_KIND, "stage": stage,
                "release": env["OXIDEX_REHEARSAL_RELEASE"], "state": "passed", "denominator": 3}
        if stage in {"read", "write"}:
            body.update(native_release=env["OXIDEX_REHEARSAL_RELEASE"],
                        native_probe_sha256=ready_probe(env["OXIDEX_REHEARSAL_RELEASE"])["probe_sha256"],
                        comparison={"kind": "oxidex_vs_native", "native_release": env["OXIDEX_REHEARSAL_RELEASE"],
                                    "matched": 3, "mismatched": 0})
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(body))
        return subprocess.CompletedProcess(argv, 0, "ok", "")

    def probe(self, *args, **kwargs):
        release = args[7]
        self.native_calls.append(release)
        return ready_probe(release)

    def execute(self):
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            return executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                    run=self.command, checkout=self.checkout)

    def test_runs_both_versions_with_owned_targets_and_per_version_oracles(self):
        self.initialize(self.config())
        journal = self.execute()
        self.assertEqual(journal["phase"], "complete")
        self.assertEqual(journal["promotion"], "forbidden")
        self.assertEqual(journal["scope"]["parity"], "per-version-read-write-rehearsed; no-promotion")
        self.assertEqual(self.native_calls, self.releases)
        self.assertEqual(len(self.checkouts), len(self.releases))
        self.assertEqual({target for _, _, target in self.calls},
                         {str(self.run_dir / "targets" / executor._safe_name(release)) for release in self.releases})
        for release in self.releases:
            self.assertEqual(journal["releases"][release]["state"], "passed")
            self.assertTrue(all(value == "passed" for value in journal["releases"][release]["stages"].values()))
            self.assertEqual(journal["releases"][release]["reports"]["read"]["denominator"], 3)
            self.assertEqual(journal["releases"][release]["reports"]["write"]["denominator"], 3)

    def test_absent_write_command_is_visible_not_parity(self):
        self.initialize(self.config(write=False))
        journal = self.execute()
        self.assertEqual(journal["phase"], "complete")
        self.assertEqual(journal["scope"]["write_acceptance"], "unsupported_for_one_or_more_releases")
        self.assertEqual(journal["scope"]["parity"], "unproven_without_all_per-release_read_and_write_acceptance")
        self.assertTrue(all(row["stages"]["write"] == "unsupported" for row in journal["releases"].values()))
        self.assertNotIn("write", [argv[0] for argv, _, _ in self.calls])

    def test_missing_denominator_fails_instead_of_counting_a_build_as_parity(self):
        self.initialize(self.config())
        original = self.command
        def no_denominator(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "read":
                report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                body = json.loads(report.read_text()); body["denominator"] = 0; report.write_text(json.dumps(body))
            return result
        self.command = no_denominator
        journal = self.execute()
        self.assertEqual(journal["phase"], "failed")
        failed = next(row for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failed["failure"]["stage"], "read")
        self.assertIn("positive denominator", failed["failure"]["detail"])

    def test_interruption_is_durable_and_never_retries_selected_work(self):
        self.initialize(self.config())
        def interrupted(argv, **kwargs):
            if argv[0] == "generate":
                raise KeyboardInterrupt()
            return self.command(argv, **kwargs)
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            with self.assertRaises(KeyboardInterrupt):
                executor.execute(self.run_dir, self.repository, self.cache, self.sources, run=interrupted, checkout=self.checkout)
        before = len(self.calls)
        recovered = executor.recover(self.run_dir, self.cache, self.sources)
        self.assertEqual(recovered["phase"], "interrupted")
        self.assertTrue(any(row["stages"]["generate"] == "interrupted" for row in recovered["releases"].values()))
        with self.assertRaisesRegex(executor.Refused, "terminal"):
            self.execute()
        self.assertEqual(len(self.calls), before)

    def test_mutated_materialized_tree_refuses_before_checkout_or_command(self):
        self.initialize(self.config())
        row = self.materialization["selected_releases"][0]
        (self.sources / row["source_directory"] / "lib/Image/ExifTool.pm").write_text("changed")
        with self.assertRaisesRegex(Exception, "verified archive"):
            self.execute()
        self.assertEqual(self.checkouts, [])
        self.assertEqual(self.calls, [])


if __name__ == "__main__":
    unittest.main()
