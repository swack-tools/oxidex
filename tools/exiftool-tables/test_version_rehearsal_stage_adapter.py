"""Offline contract tests for the concrete version-rehearsal adapter.

The command runner is fake on purpose: these tests prove orchestration and
identity checks without running regeneration, Cargo, or a corpus.
"""
from __future__ import annotations
import argparse, hashlib, json
from pathlib import Path
import subprocess, sys
from tempfile import TemporaryDirectory
import unittest

HERE = Path(__file__).resolve().parent; sys.path.insert(0, str(HERE))
import artifacts
import version_rehearsal_stage_adapter as adapter

COMMIT = "a" * 40


class AdapterTests(unittest.TestCase):
    def setUp(self):
        self.temp = TemporaryDirectory(); self.addCleanup(self.temp.cleanup); self.root = Path(self.temp.name)
        self.checkout = self.root / "checkout"; self.checkout.mkdir()
        (self.checkout / ".exiftool-version").write_text("13.59\n")
        (self.checkout / "tools/exiftool-tables").mkdir(parents=True)
        (self.checkout / "tools/exiftool-tables/regen-all.sh").write_text("#!/bin/sh\n")
        for item in artifacts.ARTIFACTS:
            path = self.checkout / item.path; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(item.key)
        self.target = self.root / "target"; self.target.mkdir(); self.reports = self.root / "reports"; self.reports.mkdir()
        self.native = self.root / "native"; (self.native / "lib/Image").mkdir(parents=True)
        (self.native / "lib/Image/ExifTool.pm").write_text("$VERSION = '11.78';\n")
        self.perl = self.root / "perl"; self.perl.write_text("perl"); self.perl.chmod(0o755)
        self.fixture = self.root / "fixture.jpg"; self.fixture.write_bytes(b"fixture")
        self.manifest = self.root / "fixtures.json"; self.manifest.write_text(json.dumps({"schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest", "fixtures": [{"path": str(self.fixture), "sha256": adapter._sha(self.fixture), "bytes": self.fixture.stat().st_size}]}))
        self.seen = []

    def args(self, stage, **extra):
        values = dict(stage=stage, checkout=str(self.checkout), target=str(self.target), report=str(self.reports / f"{stage}.json"), release="11.78", source_commit=COMMIT, native_source=str(self.native), native_lib=str(self.native / "lib"), native_perl=str(self.perl), fixture_manifest=str(self.manifest), native_probe_sha256="b" * 64)
        values.update(extra); return argparse.Namespace(**values)

    def fake_run(self, argv, **kwargs):
        self.seen.append((argv, kwargs["env"]))
        if argv[0] == "git":
            if argv[-2:] == ["rev-parse", "HEAD"]: return subprocess.CompletedProcess(argv, 0, COMMIT + "\n", "")
            if argv[-2:] == ["status", "--porcelain=v1"]: return subprocess.CompletedProcess(argv, 0, "", "")
            if argv[-2:] == ["diff", "--name-only"]: return subprocess.CompletedProcess(argv, 0, ".exiftool-version\n", "")
        if argv[0] == "bash": return subprocess.CompletedProcess(argv, 0, "regen", "")
        if argv[0] == "cargo":
            binary = self.target / "debug/oxidex"; binary.parent.mkdir(parents=True, exist_ok=True); binary.write_bytes(b"binary")
            return subprocess.CompletedProcess(argv, 0, json.dumps({"reason": "compiler-artifact", "target": {"name": "oxidex"}, "executable": str(binary)}) + "\n", "")
        if argv[0] == sys.executable:
            output = Path(argv[argv.index("--json-out") + 1]); output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps({"per_format": {"JPEG": {"matched": 2, "value_diff": 0, "missing": 0, "renames": 0, "extra": 0}}}))
            return subprocess.CompletedProcess(argv, 0, "compared", "")
        raise AssertionError(argv)

    def test_selected_release_uses_sanctioned_regen_and_explicit_native_environment(self):
        result = adapter.generate(self.args("generate"), run=self.fake_run)
        self.assertEqual(result["state"], "passed")
        self.assertEqual(result["native_identity"]["release"], "11.78")
        self.assertEqual(result["clean_source_before"]["git_status"], "clean")
        self.assertEqual((self.checkout / ".exiftool-version").read_text(), "11.78\n")
        bash, env = next(row for row in self.seen if row[0][0] == "bash")
        self.assertEqual(Path(bash[-1]).resolve(), self.checkout.resolve() / "tools/exiftool-tables/regen-all.sh")
        self.assertEqual(Path(env["EXIFTOOL_PERL"]).resolve(), self.perl.resolve()); self.assertEqual(Path(env["OXIDEX_EXIFTOOL_LIB"]).resolve(), (self.native / "lib").resolve()); self.assertEqual(Path(env["OXIDEX_ET_CACHE"]).resolve(), (self.target / "exiftool-cache").resolve()); self.assertEqual(Path(env["CARGO_TARGET_DIR"]).resolve(), self.target.resolve())
        self.assertEqual(len(result["generated_artifacts"]), len(artifacts.ARTIFACTS))

    def test_build_and_actual_read_bind_binary_fixture_and_zero_mismatches(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        built = adapter.build(self.args("build"), run=self.fake_run)
        read = adapter.read(self.args("read"), run=self.fake_run)
        self.assertEqual(built["binary"]["sha256"], read["binary"]["sha256"])
        self.assertEqual(read["state"], "passed"); self.assertEqual(read["comparison"], {"kind": "oxidex_vs_native", "native_release": "11.78", "matched": 2, "mismatched": 0})
        self.assertEqual(read["fixtures"]["entries"][0]["sha256"], adapter._sha(self.fixture))
        self.assertTrue(any(row[0][0] == sys.executable and "conformance.py" in row[0][1] for row in self.seen))

    def test_nonzero_actual_mismatch_is_a_failed_report_not_a_pass(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def mismatch(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable:
                output = Path(argv[argv.index("--json-out") + 1]); data = json.loads(output.read_text()); data["per_format"]["JPEG"]["missing"] = 1; output.write_text(json.dumps(data))
            return result
        result = adapter.read(self.args("read"), run=mismatch)
        self.assertEqual(result["state"], "failed"); self.assertEqual(result["comparison"]["mismatched"], 1)

    def test_changed_fixture_or_binary_refuses_before_comparison(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        self.fixture.write_bytes(b"changed")
        with self.assertRaisesRegex(adapter.Refused, "fixture changed"):
            adapter.read(self.args("read"), run=self.fake_run)
        self.fixture.write_bytes(b"fixture")
        built = json.loads((self.reports / "build.json").read_text()); Path(built["binary"]["path"]).write_bytes(b"replaced")
        with self.assertRaisesRegex(adapter.Refused, "executable changed"):
            adapter.read(self.args("read"), run=self.fake_run)

    def test_write_is_explicitly_unsupported(self):
        result = adapter.write(self.args("write"))
        self.assertEqual(result["state"], "unsupported")
        self.assertIn("not implemented", result["reason"])


if __name__ == "__main__": unittest.main()
