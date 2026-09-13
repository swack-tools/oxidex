#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import version_rehearsal_native_oracle as oracle

class NativeOracleTests(unittest.TestCase):
    def test_case_refuses_path_injection_and_requires_both_phases(self):
        with self.assertRaisesRegex(oracle.Refused, "filesystem paths"):
            oracle._case({"name":"x", "fixture":"x", "read":{"args":["/tmp/x"],"expectation":"success"}, "write":{"args":["-Comment=x"],"expectation":"success"}})
        with self.assertRaisesRegex(oracle.Refused, "requires success"):
            oracle._case({"name":"x", "fixture":"x", "read":{"args":["-s"],"expectation":"success"}, "write":{"args":["-x"],"expectation":"unknown"}})

    def test_capability_records_missing_module_separately(self):
        def fake(argv, **kwargs):
            return subprocess.CompletedProcess(argv, 1 if "-MArchive::Zip" in argv else 0, "", "missing")
        row = oracle._capability(Path(sys.executable), fake)
        self.assertFalse(row["available"])
        self.assertEqual(row["modules"][0]["exit"], 1)

    def test_run_records_process_failure(self):
        result = oracle._run(["fake", "-ver"], lambda argv, **kwargs: subprocess.CompletedProcess(argv, 17, "out", "err"))
        self.assertEqual((result["exit"], result["stdout"], result["stderr"]), (17, "out", "err"))

    def test_report_is_immutable_and_rejects_rehashed_mutation(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "probe.json"
            payload = {"schema": 1, "kind": oracle.KIND, "state": "failed"}
            report = {**payload, "probe_sha256": oracle.catalog_stage.sha256_json(payload)}
            oracle.write_probe_report(path, report)
            with self.assertRaisesRegex(oracle.Refused, "already exists"):
                oracle.write_probe_report(path, report)

if __name__ == "__main__":
    unittest.main()
