"""Offline contract tests for the concrete version-rehearsal adapter.

The command runner is fake on purpose: these tests prove orchestration and
identity checks without running regeneration, Cargo, or a corpus.
"""
from __future__ import annotations
import argparse, hashlib, json, os, time
from dataclasses import dataclass
from pathlib import Path
import subprocess, sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent; sys.path.insert(0, str(HERE))
import artifacts
import version_rehearsal_stage_adapter as adapter

COMMIT = "a" * 40


@dataclass(frozen=True)
class V4Target:
    raw_tag_id: int
    name: str
    table_group0: str
    physical_write_group: str
    wire_format: str

    @property
    def case_family(self) -> str:
        return "native_string_scalar" if self.wire_format == "string" else "native_unsigned_numeric_scalar"

    @property
    def qualifiers(self) -> tuple[str, str]:
        return (f"{self.table_group0}:{self.name}", f"{self.physical_write_group}:{self.name}")


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
        self.native = self.root / "native"; (self.native / "lib/Image/ExifTool").mkdir(parents=True)
        (self.native / "lib/Image/ExifTool.pm").write_text("$VERSION = '11.78';\n")
        (self.native / "lib/Image/ExifTool/Writer.pl").write_text("package Image::ExifTool; 1;\n")
        self.perl = self.root / "perl"; self.perl.write_text("perl"); self.perl.chmod(0o755)
        self.fixture = self.root / "fixture.jpg"; self.fixture.write_bytes(b"fixture")
        self.manifest = self.root / "fixtures.json"; self.manifest.write_text(json.dumps({"schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest", "fixtures": [{"path": str(self.fixture), "sha256": adapter._sha(self.fixture), "bytes": self.fixture.stat().st_size}]}))
        self.jpeg = self.root / "write.jpg"; self.jpeg.write_bytes(b"\xff\xd8fixture")
        self.write_manifest = self.root / "write-fixtures.json"; self.write_manifest.write_text(json.dumps({"schema": 1, "kind": "oxidex_version_rehearsal_write_fixture_manifest", "fixtures": [{"path": str(self.jpeg), "sha256": adapter._sha(self.jpeg), "bytes": self.jpeg.stat().st_size}]}))
        self.seen = []; self.diff_output = ".exiftool-version\n"
        self.source_targets = tuple(
            V4Target(0x013c + index, f"StringTarget{index}", "EXIF", "IFD0", "string")
            for index in range(9)
        ) + tuple(
            V4Target(0x0200 + index, f"NumericTarget{index}", "EXIF", "IFD0", "int16u")
            for index in range(6)
        )
        self.generated_targets = patch.object(adapter, "generated_targets", return_value=self.source_targets)
        self.mock_generated_targets = self.generated_targets.start()
        self.addCleanup(self.generated_targets.stop)
        self.matrix_contract = patch.multiple(
            adapter,
            explicit_directory_operands=lambda **_kwargs: ("IFD0", "IFD1"),
            selected_qualifiers=lambda target, directories: target.qualifiers + tuple(
                f"{directory}:{target.name}" for directory in directories
                if target.table_group0 == "EXIF" and directory != target.physical_write_group),
            directory_path=lambda _target, qualifier, _directories: ("NextIFD",) if qualifier.startswith("IFD1:") else (),
            case_inputs=lambda target: ({f"string_{index}": f"string-{index}".encode() for index in range(8)}
                                        if target.case_family == "native_string_scalar"
                                        else {f"numeric_{index}": str(index + 1).encode() for index in range(7)}),
            matrix_inputs=lambda target: ({**({f"string_{index}": f"string-{index}".encode() for index in range(8)}
                                              if target.case_family == "native_string_scalar"
                                              else {f"numeric_{index}": str(index + 1).encode() for index in range(7)}),
                                           **({} if target.case_family == "native_string_scalar"
                                              else {f"extended_{index}": f"extended-{index}".encode() for index in range(4)})}),
            public_scalar=lambda target, operation, value: (
                "undefined" if value is None else "float" if operation == "extended_0"
                else "integer" if target.case_family == "native_unsigned_numeric_scalar" else "bytes"),
            create=True,
        )
        self.matrix_contract.start(); self.addCleanup(self.matrix_contract.stop)

    def args(self, stage, **extra):
        manifest = self.write_manifest if stage == "write" else self.manifest
        values = dict(stage=stage, checkout=str(self.checkout), target=str(self.target), report=str(self.reports / f"{stage}.json"), release="11.78", source_commit=COMMIT, native_source=str(self.native), native_lib=str(self.native / "lib"), native_perl=str(self.perl), fixture_manifest=str(manifest), native_probe_sha256="b" * 64)
        values.update(extra); return argparse.Namespace(**values)

    def fake_run(self, argv, **kwargs):
        self.seen.append((argv, kwargs["env"]))
        if argv[0] == "git":
            if argv[-2:] == ["rev-parse", "HEAD"]: return subprocess.CompletedProcess(argv, 0, COMMIT + "\n", "")
            if argv[-2:] == ["status", "--porcelain=v1"]: return subprocess.CompletedProcess(argv, 0, "", "")
            if argv[-2:] == ["diff", "--name-only"]: return subprocess.CompletedProcess(argv, 0, self.diff_output, "")
        if argv[0] == "bash": return subprocess.CompletedProcess(argv, 0, "regen", "")
        if argv[0] == "cargo":
            test = argv[1] == "test"
            binary = self.target / ("debug/deps/oxidex-writer-test" if test else "debug/oxidex")
            binary.parent.mkdir(parents=True, exist_ok=True); binary.write_bytes(b"writer" if test else b"binary"); binary.chmod(0o755)
            return subprocess.CompletedProcess(argv, 0, json.dumps({"reason": "compiler-artifact", "manifest_path": str(self.checkout / "Cargo.toml"), "profile": {"test": test}, "target": {"name": "oxidex", "kind": ["lib"] if test else ["bin"]}, "executable": str(binary)}) + "\n", "")
        if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
            output = Path(argv[argv.index("--output") + 1]); output.parent.mkdir(parents=True, exist_ok=True)
            writer = Path(argv[argv.index("--test-binary") + 1]); ledger = Path(argv[argv.index("--ledger") + 1]); rules = Path(argv[argv.index("--rules") + 1])
            perl = argv[argv.index("--perl") + 1]; native_lib = argv[argv.index("--lib") + 1]
            pin = Path(argv[argv.index("--rehearsal-pin") + 1])
            directories = ("IFD0", "IFD1")
            cohort, rows = [], []
            for target in self.source_targets:
                qualifiers = target.qualifiers + tuple(f"{directory}:{target.name}" for directory in directories
                                                       if directory != target.physical_write_group)
                inputs = ({f"string_{index}": f"string-{index}".encode() for index in range(8)}
                          if target.case_family == "native_string_scalar"
                          else {**{f"numeric_{index}": str(index + 1).encode() for index in range(7)},
                                **{f"extended_{index}": f"extended-{index}".encode() for index in range(4)}})
                scalars = {operation: ("undefined" if value is None else "float" if operation == "extended_0"
                                       else "integer" if target.case_family == "native_unsigned_numeric_scalar" else "bytes")
                           for operation, value in inputs.items()}
                cohort.append({"raw_tag_id": target.raw_tag_id, "name": target.name, "table_group0": target.table_group0,
                               "physical_write_group": target.physical_write_group, "wire_format": target.wire_format,
                               "case_family": target.case_family, "cases": list(inputs),
                               "case_inputs": {operation: {"value_hex": None if value is None else value.hex(),
                                                            "public_scalar": scalars[operation]} for operation, value in inputs.items()},
                               "qualifiers": list(qualifiers)})
                for carrier in ("tiff_little", "tiff_big", "jpeg"):
                    for requested_name in qualifiers:
                        for operation, value in inputs.items():
                            request_output = str(output.parent / f"{carrier}-{requested_name}-{operation}.jpg")
                            selected = requested_name.split(":", 1)[0] in directories and requested_name.startswith("IFD1:")
                            extended = operation.startswith("extended_")
                            rows.append({"state": "passed", "carrier": carrier,
                                         "target": {"raw_tag_id": target.raw_tag_id, "name": target.name,
                                                    "table_group0": target.table_group0, "physical_write_group": target.physical_write_group,
                                                    "wire_format": target.wire_format},
                                         "case_family": target.case_family, "requested_name": requested_name,
                                         "target_directory": ["NextIFD"] if selected else [],
                                         "coverage_family": ("extended_" if extended else "")
                                                            + ("selected_directory_" if selected else "baseline_") + target.case_family,
                                         "public_scalar": scalars[operation], "operation": operation,
                                         "output": request_output,
                                         "driver_result": {"ok": True, "warnings": [], "output": request_output}})
            output.write_text(json.dumps({"instrument": "generated_scalar_write_matrix_v4", "route": "public-api",
                "native_identity": {"command": [perl, "-I" + native_lib], "result": {"exiftool_version": "11.78",
                                    "source_sha256": adapter._native_writer_sources(Path(native_lib))}},
                "contract": {"mode": "selected-release-rehearsal", "release": "11.78", "ledger_exiftool_version": "11.78",
                             "pin": str(pin.resolve()), "pin_sha256": adapter._sha(pin)},
                "source_commit": COMMIT, "test_binary_path": str(writer), "test_binary_sha256": adapter._sha(writer), "ledger_sha256": adapter._sha(ledger), "rules_sha256": adapter._sha(rules),
                "cohort": cohort, "explicit_directories": list(directories),
                "declared_by_case_family": {family: sum(row["case_family"] == family for row in rows)
                                             for family in sorted({row["case_family"] for row in rows})},
                "declared_by_coverage_family": {family: sum(row["coverage_family"] == family for row in rows)
                                                 for family in sorted({row["coverage_family"] for row in rows})},
                "declared_by_qualifier": {qualifier: sum(row["requested_name"].split(":", 1)[0] == qualifier for row in rows)
                                           for qualifier in sorted({row["requested_name"].split(":", 1)[0] for row in rows})},
                "declared": len(rows), "passed": len(rows), "rows": rows}))
            return subprocess.CompletedProcess(argv, 0, "matrix", "")
        if argv[0] == sys.executable:
            output = Path(argv[argv.index("--json-out") + 1]); output.parent.mkdir(parents=True, exist_ok=True)
            corpus = Path(argv[2]); fixture = next(corpus.iterdir())
            output.write_text(json.dumps({"per_format": {"JPEG": {"files": 1, "matched": 2, "value_diff": 0, "missing": 0, "renames": 0, "extra": 0}}, "per_file": {str(fixture): {"format": "JPEG"}}}))
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
        self.assertEqual(env["OXIDEX_ALLOW_DIRTY_TREE"], "1")
        self.assertEqual(len(result["generated_artifacts"]), len(artifacts.ARTIFACTS))

    def test_build_and_actual_read_bind_binary_fixture_and_zero_mismatches(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        built = adapter.build(self.args("build"), run=self.fake_run)
        read = adapter.read(self.args("read"), run=self.fake_run)
        self.assertEqual(built["binary"]["sha256"], read["binary"]["sha256"])
        self.assertEqual(read["state"], "passed"); self.assertEqual(read["comparison"], {"kind": "oxidex_vs_native", "native_release": "11.78", "matched": 2, "mismatched": 0})
        self.assertEqual(read["fixtures"]["entries"][0]["sha256"], adapter._sha(self.fixture))
        self.assertTrue(any(row[0][0] == sys.executable and "conformance.py" in row[0][1] for row in self.seen))

    def test_build_records_distinct_cli_and_writer_driver(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        built = adapter.build(self.args("build"), run=self.fake_run)
        self.assertEqual(built["denominator"], 2)
        self.assertNotEqual(built["binary"]["path"], built["writer_binary"]["path"])
        adapter.executor._require_binary_proof(built, self.target, "writer_binary")
        Path(built["writer_binary"]["path"]).write_bytes(b"changed")
        with self.assertRaisesRegex(adapter.executor.Refused, "no longer matches"):
            adapter.executor._require_binary_proof(built, self.target, "writer_binary")

    def test_missing_test_profile_cannot_substitute_the_cli_for_writer_driver(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        def wrong_profile(argv, **kwargs):
            result = self.fake_run(argv, **kwargs)
            if argv[:2] == ["cargo", "test"]:
                row = json.loads(result.stdout); row["profile"]["test"] = False
                return subprocess.CompletedProcess(argv, 0, json.dumps(row), "")
            return result
        with self.assertRaisesRegex(adapter.Refused, "one isolated oxidex lib"):
            adapter.build(self.args("build"), run=wrong_profile)
        self.assertFalse((self.reports / "build.json").exists())

    def test_foreign_manifest_cannot_supply_rehearsal_executable(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        def foreign_package(argv, **kwargs):
            result = self.fake_run(argv, **kwargs)
            if argv[0] == "cargo":
                row = json.loads(result.stdout); row["manifest_path"] = str(self.root / "other/Cargo.toml")
                return subprocess.CompletedProcess(argv, 0, json.dumps(row), "")
            return result
        with self.assertRaisesRegex(adapter.Refused, "one isolated oxidex bin"):
            adapter.build(self.args("build"), run=foreign_package)

    def test_failed_writer_driver_build_never_publishes_build_success(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        def fails_driver(argv, **kwargs):
            if argv[:2] == ["cargo", "test"]:
                return subprocess.CompletedProcess(argv, 1, "", "driver failed")
            return self.fake_run(argv, **kwargs)
        with self.assertRaisesRegex(adapter.Refused, "writer-driver compilation failed"):
            adapter.build(self.args("build"), run=fails_driver)
        self.assertFalse((self.reports / "build.json").exists())
        raw = json.loads((self.reports / "raw/build-command.json").read_text())
        self.assertEqual(raw["state"], "failed")
        self.assertEqual(raw["commands"][-1]["exit"], 1)

    def test_generate_accepts_an_owned_checkout_already_pinned_to_selected_release(self):
        (self.checkout / ".exiftool-version").write_text("11.78\n"); self.diff_output = ""
        result = adapter.generate(self.args("generate"), run=self.fake_run)
        self.assertEqual(result["state"], "passed")

    def test_staged_fixture_mutation_or_incomplete_per_file_report_refuses(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def alters_staged(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable:
                corpus = Path(argv[2]); staged = next(corpus.iterdir()); staged.chmod(0o644); staged.write_bytes(b"changed")
            return result
        with self.assertRaisesRegex(adapter.Refused, "staged fixture changed"):
            adapter.read(self.args("read"), run=alters_staged)

    def test_read_refuses_conformance_that_omits_a_staged_fixture(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def incomplete(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable:
                output = Path(argv[argv.index("--json-out") + 1]); data = json.loads(output.read_text()); data["per_file"] = {}; output.write_text(json.dumps(data))
            return result
        with self.assertRaisesRegex(adapter.Refused, "exactly the staged fixture manifest"):
            adapter.read(self.args("read"), run=incomplete)

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

    def test_write_uses_proven_writer_binary_and_dedicated_staged_jpeg_scope(self):
        adapter.generate(self.args("generate"), run=self.fake_run); built = adapter.build(self.args("build"), run=self.fake_run)
        result = adapter.write(self.args("write"), run=self.fake_run)
        self.assertEqual(result["state"], "passed")
        self.assertEqual(result["writer_binary"], built["writer_binary"])
        self.assertEqual(result["comparison"], {"kind": "oxidex_vs_native", "native_release": "11.78", "matched": 1242, "mismatched": 0})
        self.assertEqual(result["write_mode"]["original_subset"], 684)
        self.assertEqual(result["write_mode"]["expanded_matrix"], 1242)
        self.assertEqual(set(result["write_mode"]["source_contract"]), {"final_ledger", "final_rules", "scalar_helper_ledger", "scalar_rules", "address_rules"})
        self.assertIn("rehearsal-write-fixtures", result["fixtures"]["entries"][0]["corpus_path"])
        matrix = next(row for row in self.seen if row[0][0] == sys.executable and row[0][1].endswith("generated_tiff_write_matrix.py"))
        self.assertIn("--rehearsal-release", matrix[0]); self.assertIn("--route", matrix[0]); self.assertEqual(matrix[0][matrix[0].index("--route") + 1], "public-api")

    def test_write_refuses_non_jpeg_or_changed_fixture_scope(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        self.jpeg.write_bytes(b"changed")
        with self.assertRaisesRegex(adapter.Refused, "fixture changed"):
            adapter.write(self.args("write"), run=self.fake_run)
        self.jpeg.write_bytes(b"not jpeg")
        self.write_manifest.write_text(json.dumps({"schema": 1, "kind": "oxidex_version_rehearsal_write_fixture_manifest", "fixtures": [{"path": str(self.jpeg), "sha256": adapter._sha(self.jpeg), "bytes": self.jpeg.stat().st_size}]}))
        with self.assertRaisesRegex(adapter.Refused, "non-JPEG"):
            adapter.write(self.args("write"), run=self.fake_run)

    def test_write_refuses_source_or_checkout_changes_after_build(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def alters_source(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
                self.jpeg.write_bytes(b"changed")
            return result
        with self.assertRaisesRegex(adapter.Refused, "fixture source changed"):
            adapter.write(self.args("write"), run=alters_source)
        self.jpeg.write_bytes(b"\xff\xd8fixture")
        (self.checkout / "unexpected.rs").write_text("changed")
        with self.assertRaisesRegex(adapter.Refused, "prior stage"):
            adapter.write(self.args("write"), run=self.fake_run)

    def test_write_refuses_v4_source_contract_change_after_matrix_execution(self):
        """A post-report helper-rule change cannot reuse a v4 matrix result."""
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def alters_v4_source(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
                (self.checkout / "src/writers/generated_scalar_rules.rs").write_text("changed-after-matrix")
            return result
        with self.assertRaisesRegex(adapter.Refused, "prior stage"):
            adapter.write(self.args("write"), run=alters_v4_source)

    def test_write_refuses_aggregate_success_without_driver_rows(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def removes_driver_evidence(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
                output = Path(argv[argv.index("--output") + 1]); data = json.loads(output.read_text())
                data["rows"] = [{"state": "passed"} for _ in range(3)]
                output.write_text(json.dumps(data))
            return result
        with self.assertRaisesRegex(adapter.Refused, "actual successful public-driver"):
            adapter.write(self.args("write"), run=removes_driver_evidence)

    def test_write_refuses_v4_report_with_a_retyped_numeric_case(self):
        """A scalar label cannot be changed without changing the source-derived case proof."""
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def retypes_numeric_case(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
                output = Path(argv[argv.index("--output") + 1]); data = json.loads(output.read_text())
                numeric = next(target for target in data["cohort"] if target["case_family"] == "native_unsigned_numeric_scalar")
                numeric["case_inputs"]["extended_0"]["public_scalar"] = "bytes"
                output.write_text(json.dumps(data))
            return result
        with self.assertRaisesRegex(adapter.Refused, "cohort differs"):
            adapter.write(self.args("write"), run=retypes_numeric_case)

    def test_write_refuses_report_that_omits_a_current_generated_target(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        # The report itself remains internally consistent for HostComputer, but
        # source operands now include a second target.  The adapter must not
        # let the report choose a smaller denominator by omitting it.
        self.mock_generated_targets.return_value = self.source_targets + (
            V4Target(0x013d, "Software", "EXIF", "IFD0", "string"),
        )
        with self.assertRaisesRegex(adapter.Refused, "source cohort"):
            adapter.write(self.args("write"), run=self.fake_run)

    def test_write_refuses_nonzero_matrix_command_and_forged_driver_success(self):
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def nonzero(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
                return subprocess.CompletedProcess(argv, 1, "matrix failed", "native mismatch")
            return result
        with self.assertRaisesRegex(adapter.Refused, "matrix command failed"):
            adapter.write(self.args("write"), run=nonzero)

        # A nonempty mapping is insufficient: adapter evidence must contain
        # the successful public driver outcome bound to the row's output.
        self.reports = self.root / "forged-driver-reports"; self.reports.mkdir()
        self.target = self.root / "forged-driver-target"; self.target.mkdir()
        adapter.generate(self.args("generate"), run=self.fake_run); adapter.build(self.args("build"), run=self.fake_run)
        def forged_driver(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
                output = Path(argv[argv.index("--output") + 1]); data = json.loads(output.read_text())
                data["rows"][0]["driver_result"] = {"ok": False, "error": "forged"}
                output.write_text(json.dumps(data))
            return result
        with self.assertRaisesRegex(adapter.Refused, "actual successful public-driver"):
            adapter.write(self.args("write"), run=forged_driver)

    def test_write_rechecks_writer_binary_after_matrix_execution(self):
        adapter.generate(self.args("generate"), run=self.fake_run); built = adapter.build(self.args("build"), run=self.fake_run)
        original = self.fake_run
        def replaces_writer_after_matrix(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == sys.executable and argv[1].endswith("generated_tiff_write_matrix.py"):
                Path(built["writer_binary"]["path"]).write_bytes(b"replaced-after-matrix")
            return result
        with self.assertRaisesRegex(adapter.Refused, "writer driver changed after build"):
            adapter.write(self.args("write"), run=replaces_writer_after_matrix)

    def test_executor_timeout_kills_adapter_nested_child_and_releases_lock(self):
        """Exercise executor -> adapter._run -> sleeping child with real PIDs."""
        lock, pid = self.root / "shared.lock", self.root / "nested.pid"
        helper = self.root / "adapter-helper.py"
        child_code = f"from pathlib import Path; import os,time; Path({str(pid)!r}).write_text(str(os.getpid())); time.sleep(30)"
        helper.write_text(
            "import os, subprocess, sys\nfrom pathlib import Path\n"
            f"sys.path.insert(0, {str(HERE)!r})\nimport version_rehearsal_stage_adapter as a\n"
            f"code = {child_code!r}\n"
            "a._run([sys.executable, '-c', code], cwd=Path.cwd(), env=dict(os.environ), run=subprocess.run)\n")
        supervisor = self.root / "executor-supervisor.py"
        supervisor.write_text(
            "import json, os, subprocess, sys\nfrom pathlib import Path\n"
            f"sys.path.insert(0, {str(HERE)!r})\nimport version_rehearsal_executor as e\n"
            "e.COMMAND_TIMEOUT_SECONDS = 1\n"
            f"with e._HostLock(Path({str(lock)!r})):\n"
            f" r=e._run_record([sys.executable, {str(helper)!r}], cwd=Path.cwd(), env=dict(os.environ), run=subprocess.run)\n"
            "print(json.dumps(r))\n")
        finished = subprocess.run([sys.executable, str(supervisor)], cwd=self.root, text=True, capture_output=True, timeout=10)
        self.assertEqual(finished.returncode, 0, finished.stderr)
        self.assertEqual(json.loads(finished.stdout)["state"], "timeout", finished.stdout + finished.stderr)
        self.assertTrue(pid.is_file(), "nested child did not start")
        child = int(pid.read_text())
        live = True
        for _ in range(20):
            try: os.kill(child, 0)
            except ProcessLookupError: live = False; break
            time.sleep(0.05)
        self.assertFalse(live, "executor timeout orphaned adapter child")
        import version_rehearsal_executor as executor
        with executor._HostLock(lock): pass

    def test_executor_timeout_reaps_late_child_after_parent_exits(self):
        """A child born after the snapshot cannot survive its parent's early exit."""
        lock, pid = self.root / "late.lock", self.root / "late.pid"
        helper = self.root / "late-helper.py"
        child_code = f"from pathlib import Path; import os,time; Path({str(pid)!r}).write_text(str(os.getpid())); time.sleep(30)"
        helper.write_text(
            "import os, subprocess, sys, time\nfrom pathlib import Path\n"
            "time.sleep(1.2)\n"
            f"child = subprocess.Popen([sys.executable, '-c', {child_code!r}], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, close_fds=True)\n"
            f"Path({str(pid)!r}).write_text(str(child.pid))\n"
            "# Exit before the executor's first cleanup grace period expires.\n")
        supervisor = self.root / "late-supervisor.py"
        supervisor.write_text(
            "import json, os, subprocess, sys\nfrom pathlib import Path\n"
            f"sys.path.insert(0, {str(HERE)!r})\nimport version_rehearsal_executor as e\n"
            "e.COMMAND_TIMEOUT_SECONDS = 1\n"
            f"with e._HostLock(Path({str(lock)!r})):\n"
            f" r=e._run_record([sys.executable, {str(helper)!r}], cwd=Path.cwd(), env=dict(os.environ), run=subprocess.run)\n"
            "print(json.dumps(r))\n")
        finished = subprocess.run([sys.executable, str(supervisor)], cwd=self.root, text=True, capture_output=True, timeout=12)
        self.assertEqual(finished.returncode, 0, finished.stderr)
        self.assertEqual(json.loads(finished.stdout)["state"], "timeout", finished.stdout + finished.stderr)
        self.assertTrue(pid.is_file(), "late child did not start")
        child = int(pid.read_text())
        for _ in range(20):
            try: os.kill(child, 0)
            except ProcessLookupError: break
            time.sleep(0.05)
        else:
            self.fail("process-group fallback left the late child live")
        import version_rehearsal_executor as executor
        with executor._HostLock(lock): pass


if __name__ == "__main__": unittest.main()
