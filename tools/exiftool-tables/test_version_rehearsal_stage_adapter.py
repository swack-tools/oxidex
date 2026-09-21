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
from types import SimpleNamespace
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent; sys.path.insert(0, str(HERE))
import artifacts
import table_modules
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


def fixture_text(artifact) -> str:
    """Placeholder contents for one declared output. A split table hub names
    its module files, as a generated one does, so the fake checkout is a
    consistent split set (artifacts.family_errors)."""
    if artifact.key in artifacts.MODULE_FAMILIES:
        return table_modules.render_hub(f"//! {artifact.key}\n", artifacts.module_stems(artifact.key), "")
    return artifact.key


class AdapterTests(unittest.TestCase):
    def setUp(self):
        self.temp = TemporaryDirectory(); self.addCleanup(self.temp.cleanup); self.root = Path(self.temp.name)
        self.checkout = self.root / "checkout"; self.checkout.mkdir()
        (self.checkout / ".exiftool-version").write_text("13.59\n")
        (self.checkout / "tools/exiftool-tables").mkdir(parents=True)
        (self.checkout / "tools/exiftool-tables/regen-all.sh").write_text("#!/bin/sh\n")
        for item in artifacts.ARTIFACTS:
            path = self.checkout / item.path; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(fixture_text(item))
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
            for index in range(13)
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
                "declared": len(rows), "passed": len(rows), "rows": rows}, sort_keys=True))
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
        self.assertEqual(len([row for row in self.seen if row[0][0] == "bash"]), 2)
        self.assertEqual(result["second_regeneration"]["state"], "clean")
        self.assertEqual(result["second_regeneration"]["first_source_tree_sha256"],
                         result["second_regeneration"]["second_source_tree_sha256"])
        self.assertEqual(result["second_regeneration"]["first_pin"],
                         result["second_regeneration"]["second_pin"])
        self.assertEqual(result["second_regeneration"]["first_artifacts"],
                         result["second_regeneration"]["second_artifacts"])
        raw = json.loads((self.reports / "raw/generate-command.json").read_text())
        self.assertEqual(set(raw), {"first", "second"})
        self.assertEqual(
            [row["path"] for row in result["generated_artifacts"]],
            [item.path for item in artifacts.inventory(self.checkout)],
        )

    def test_first_generation_failure_persists_raw_command_receipt(self):
        def fails_first_generation(argv, **kwargs):
            if argv[0] == "bash":
                self.seen.append((argv, kwargs["env"]))
                return subprocess.CompletedProcess(argv, 17, "first stdout", "first stderr")
            return self.fake_run(argv, **kwargs)
        with self.assertRaisesRegex(adapter.Refused, "sanctioned regen-all.sh failed"):
            adapter.generate(self.args("generate"), run=fails_first_generation)
        self.assertEqual(len([row for row in self.seen if row[0][0] == "bash"]), 1)
        raw = json.loads((self.reports / "raw/generate-command.json").read_text())
        self.assertEqual(raw["first"]["exit"], 17)
        self.assertEqual(raw["first"]["stdout"], "first stdout")
        self.assertEqual(raw["first"]["stderr"], "first stderr")

    def test_second_generation_interruption_preserves_first_command_receipt(self):
        calls = 0
        def interrupts_second_generation(argv, **kwargs):
            nonlocal calls
            if argv[0] == "bash":
                calls += 1
                if calls == 2:
                    raise KeyboardInterrupt("second generation interrupted")
            return self.fake_run(argv, **kwargs)
        with self.assertRaisesRegex(KeyboardInterrupt, "second generation interrupted"):
            adapter.generate(self.args("generate"), run=interrupts_second_generation)
        raw = json.loads((self.reports / "raw/generate-command.json").read_text())
        self.assertEqual(set(raw), {"first"})
        self.assertEqual(raw["first"]["state"], "ok")

    def test_second_in_place_regeneration_must_be_clean(self):
        calls = 0
        generated = self.checkout / artifacts.ARTIFACTS[0].path
        def changes_only_on_second(argv, **kwargs):
            nonlocal calls
            result = self.fake_run(argv, **kwargs)
            if argv[0] == "bash":
                calls += 1
                if calls == 2:
                    generated.write_text(generated.read_text() + "second-pass-drift")
            return result
        with self.assertRaisesRegex(adapter.Refused, "second regeneration was not clean"):
            adapter.generate(self.args("generate"), run=changes_only_on_second)
        self.assertEqual(calls, 2)

    def test_build_and_actual_read_bind_binary_fixture_and_zero_mismatches(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        built = adapter.build(self.args("build"), run=self.fake_run)
        read = adapter.read(self.args("read"), run=self.fake_run)
        self.assertEqual(built["binary"]["sha256"], read["binary"]["sha256"])
        self.assertEqual(read["state"], "passed"); self.assertEqual(read["comparison"], {"kind": "oxidex_vs_native", "native_release": "11.78", "matched": 2, "mismatched": 0})
        self.assertEqual(read["classification_counts"], {
            "matched": 2, "value_diff": 0, "missing": 0, "renames": 0, "extra": 0,
        })
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
        self.assertEqual(result["comparison"], {"kind": "oxidex_vs_native", "native_release": "11.78", "matched": 1530, "mismatched": 0})
        self.assertEqual(result["write_mode"]["original_subset"], 876)
        self.assertEqual(result["write_mode"]["expanded_matrix"], 1530)
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
        with self.assertRaisesRegex(adapter.Refused, "cohort differs"):
            adapter.write(self.args("write"), run=self.fake_run)

    def test_write_counts_follow_changed_authenticated_source_family_totals(self):
        # A different release may emit fewer string rows and more numeric rows.
        # Every generated row/case is still mandatory in the report; no fixed
        # historical tag count is itself a source-authentication condition.
        self.source_targets = self.source_targets[:2] + self.source_targets[-6:]
        self.mock_generated_targets.return_value = self.source_targets
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)
        result = adapter.write(self.args("write"), run=self.fake_run)
        self.assertEqual(result["comparison"]["matched"], 2 * 72 + 6 * 99)
        self.assertEqual(result["write_mode"]["original_subset"], 2 * 48 + 6 * 42)
        self.assertEqual(result["write_mode"]["expanded_matrix"], 2 * 72 + 6 * 99)

    def test_source_contract_refuses_empty_duplicate_or_incomplete_baselines(self):
        paths = adapter._matrix_source_artifacts(self.checkout)
        for targets in ((), self.source_targets + (self.source_targets[0],)):
            with self.subTest(targets=len(targets)), patch.object(adapter, "generated_targets", return_value=targets):
                with self.assertRaisesRegex(adapter.Refused, "empty|duplicate"):
                    adapter._matrix_contract(paths)
        for name, replacement in (
            ("case_inputs", lambda target: {}),
            ("matrix_inputs", lambda target: {}),
            ("matrix_inputs", lambda target: {key: b"changed" for key in adapter.case_inputs(target)}),
            ("selected_qualifiers", lambda target, dirs: target.qualifiers[:1]),
            ("selected_qualifiers", lambda target, dirs: target.qualifiers + target.qualifiers[:1]),
        ):
            with self.subTest(operand=name), patch.object(adapter, name, replacement):
                with self.assertRaisesRegex(adapter.Refused, "target contract"):
                    adapter._matrix_contract(paths)

    def test_report_refuses_missing_duplicate_fabricated_rows_and_changed_totals(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        built = adapter.build(self.args("build"), run=self.fake_run)
        result = adapter.write(self.args("write"), run=self.fake_run)
        report = Path(result["matrix_reports"][0]["path"])
        original = json.loads(report.read_text())
        paths = adapter._matrix_source_artifacts(self.checkout)
        for mutation in ("missing", "duplicate", "fabricated", "family_total"):
            changed = json.loads(json.dumps(original))
            if mutation == "missing":
                changed["rows"].pop()
            elif mutation == "duplicate":
                changed["rows"][0] = changed["rows"][1]
            elif mutation == "fabricated":
                changed["rows"][0]["target"]["name"] = "FabricatedSourceTarget"
            else:
                changed["declared_by_case_family"]["native_string_scalar"] -= 1
            if mutation != "family_total":
                rows = changed["rows"]
                changed["declared"] = changed["passed"] = len(rows)
                changed["declared_by_case_family"] = dict(adapter.Counter(row["case_family"] for row in rows))
                changed["declared_by_coverage_family"] = dict(adapter.Counter(row["coverage_family"] for row in rows))
                changed["declared_by_qualifier"] = dict(adapter.Counter(row["requested_name"].split(":", 1)[0] for row in rows))
            report.write_text(json.dumps(changed))
            with self.subTest(mutation=mutation), self.assertRaisesRegex(adapter.Refused, "not bound"):
                adapter._matrix_report(report, args=self.args("write"), native_perl=self.perl,
                                       native_lib=self.native / "lib", writer=built["writer_binary"],
                                       paths=paths, pin=self.checkout / ".exiftool-version")

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


class LiveInventoryProofTests(unittest.TestCase):
    """The generate/build artifact proof hashes the release's regenerated
    inventory, not the pinned release's module list (#823's split tables:
    11.78 drops binary/dji.rs and adds ifd/json.rs)."""

    def test_artifact_rows_hash_the_regenerated_module_set(self):
        with TemporaryDirectory() as temp:
            checkout = Path(temp)
            for item in artifacts.ARTIFACTS:
                path = checkout / item.path; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(fixture_text(item))
            (checkout / "src/exiftool_tables/binary/dji.rs").unlink()
            binary = [s for s in artifacts.module_stems("binary") if s != "dji"]
            (checkout / "src/exiftool_tables/binary/mod.rs").write_text(table_modules.render_hub("//! binary\n", binary, ""))
            (checkout / "src/exiftool_tables/ifd/json.rs").write_text("json")
            ifd = sorted([*artifacts.module_stems("ifd"), "json"])
            (checkout / "src/exiftool_tables/ifd/mod.rs").write_text(table_modules.render_hub("//! ifd\n", ifd, ""))
            rows = adapter._artifact_rows(checkout)
            paths = [row["path"] for row in rows]
            self.assertEqual(paths, [item.path for item in artifacts.inventory(checkout)])
            self.assertIn("src/exiftool_tables/ifd/json.rs", paths)
            self.assertNotIn("src/exiftool_tables/binary/dji.rs", paths)
            json_row = next(row for row in rows if row["path"] == "src/exiftool_tables/ifd/json.rs")
            self.assertEqual(json_row["sha256"], hashlib.sha256(b"json").hexdigest())
            self.assertEqual(adapter._validate_artifacts(checkout, rows), rows)
            # A proof taken against the pinned (13.59) list does not validate here.
            pinned = [{"path": item.path, "sha256": "0" * 64, "bytes": 0} for item in artifacts.ARTIFACTS]
            with self.assertRaises(adapter.Refused):
                adapter._validate_artifacts(checkout, pinned)
            # And the proof binds content: a changed member no longer validates.
            (checkout / "src/exiftool_tables/ifd/json.rs").write_text("changed")
            with self.assertRaisesRegex(adapter.Refused, "no longer matches"):
                adapter._validate_artifacts(checkout, rows)

    def test_generated_refusals_are_explicitly_counted_by_artifact_and_json_path(self):
        with TemporaryDirectory() as temporary:
            checkout = Path(temporary)
            ledger = checkout / "generated-ledger.json"
            ledger.write_text(json.dumps({
                "stats": {"refused": 2, "by_kind": {"refused": 2},
                          "omitted_rows": [1, 2, 3], "rows_omitted": 3,
                          "top_refused_expressions": ["summary"]},
                "rows": [{"withheld": ["reason"]}, {"withheld": []}],
                "source_report": {"source_row_report": {"omitted_rows": [1, 2, 3],
                                                          "rows_omitted": 3}},
            }))
            with patch.object(adapter.artifacts, "inventory",
                              return_value=[SimpleNamespace(path="generated-ledger.json")]):
                result = adapter.generated_refusal_counts(checkout)
            self.assertEqual(result["total"], 5)
            self.assertEqual(
                {(row["json_path"], row["count"]) for row in result["counters"]},
                {("stats.refused", 2), ("stats.rows_omitted", 3)},
            )


class CurrentGeneratedMatrixContractTests(unittest.TestCase):
    def test_actual_committed_operands_define_complete_matrix_and_family_counts(self):
        # Read the real joined ledger/Rust operands. This is a compiler-contract
        # test, not a native/public execution claim, and has no tag-count pin.
        paths = adapter._matrix_source_artifacts(HERE.parents[1])
        targets = adapter.generated_targets(paths["final_ledger"], paths["final_rules"])
        cohort, directories, expected, baseline = adapter._matrix_contract(paths)
        self.assertEqual(len(cohort), len(targets))
        counts = [3 * len(adapter.matrix_inputs(target))
                  * len(adapter.selected_qualifiers(target, directories)) for target in targets]
        self.assertEqual(len(expected), sum(counts))
        self.assertEqual(baseline, sum(3 * len(target.qualifiers) * len(adapter.case_inputs(target))
                                       for target in targets))
        families, coverage, qualifiers = adapter._matrix_counts(expected)
        for family in {target.case_family for target in targets}:
            self.assertEqual(families[family], sum(count for target, count in zip(targets, counts)
                                                  if target.case_family == family))
        self.assertEqual(sum(coverage.values()), len(expected))
        self.assertEqual(sum(qualifiers.values()), len(expected))
        self.assertGreater(baseline, 0)


if __name__ == "__main__": unittest.main()
