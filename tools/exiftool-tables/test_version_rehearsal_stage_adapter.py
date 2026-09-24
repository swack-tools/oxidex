"""Offline contract tests for the concrete version-rehearsal adapter.

The command runner is fake on purpose: these tests prove orchestration and
identity checks without running regeneration, Cargo, or a corpus.
"""
from __future__ import annotations
import argparse, hashlib, json, os, time
from dataclasses import dataclass
from pathlib import Path
import shutil, subprocess, sys
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



# Captured from real `cargo test --workspace --all-features --no-fail-fast`
# runs (stderr merged into stdout, CARGO_TERM_COLOR=never) on this host:
# cargo's `Running`/`Doc-tests` line precedes each target's own output.
SUITE_TESTS_OUTPUT = '    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.00s\n     Running unittests src/lib.rs (target/debug/deps/tiny-63dad69917d0d146)\n\nrunning 3 tests\ntest t::b ... ignored\ntest t::c ... ok\ntest t::a ... ok\n\ntest result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n     Running unittests src/main.rs (target/debug/deps/tiny-4651c4a6039ea66e)\n\nrunning 1 test\ntest m ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n     Running tests/it.rs (target/debug/deps/it-50a268676428d270)\n\nrunning 1 test\ntest i ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n   Doc-tests tiny\n\nrunning 1 test\ntest src/lib.rs - one (line 1) ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s\n\n'
SUITE_FAILED_OUTPUT = "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.00s\n     Running unittests src/lib.rs (target/debug/deps/tiny-63dad69917d0d146)\n\nrunning 3 tests\ntest t::b ... ignored\ntest t::a ... ok\ntest t::c ... FAILED\n\nfailures:\n\n---- t::c stdout ----\n\nthread 't::c' (5090742) panicked at src/lib.rs:9:62:\nboom\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n\nfailures:\n    t::c\n\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\nerror: test failed, to rerun pass `--lib`\n     Running unittests src/main.rs (target/debug/deps/tiny-4651c4a6039ea66e)\n\nrunning 1 test\ntest m ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n     Running tests/it.rs (target/debug/deps/it-50a268676428d270)\n\nrunning 1 test\ntest i ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n   Doc-tests tiny\n\nrunning 1 test\ntest src/lib.rs - one (line 1) ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s\n\nerror: 1 target failed:\n    `--lib`\n"
# Edition 2024 merges doc tests: one `Doc-tests` heading carries a merged
# block and a standalone block, each with its own start and summary.
SUITE_EDITION2024_OUTPUT = '    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.27s\n     Running unittests src/lib.rs (target/debug/deps/tiny24-0f7cd0931238ab25)\n\nrunning 1 test\ntest t::a ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n     Running tests/it.rs (target/debug/deps/it-db7a870656a72ec5)\n\nrunning 1 test\ntest i ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n   Doc-tests tiny24\n\nrunning 3 tests\ntest src/lib.rs - one (line 13) - should panic ... ok\ntest src/lib.rs - one (line 5) ... ok\ntest src/lib.rs - one (line 1) ... ok\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n\nrunning 1 test\ntest src/lib.rs - one (line 9) ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s\n\nall doctests ran in 0.38s; merged doctests compilation took 0.19s\n'
# Shape of this repository's real output: tests that re-exec their own binary
# with a filter and inherited stdout interleave nested libtest runs, including
# garbled lines, inside the outer target's output (excerpt of the 13.59 run).
SUITE_NESTED_OUTPUT = """     Running unittests src/lib.rs (target/debug/deps/oxidex-1a)

running 4 tests
test fixtures::tests::absent_optional_fixture_has_no_path ... ok

running 1 test

running 1 test
test fixtures::pinned_fixtures::environment_tests::absent_fallback_is_optional ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out; finished in 0.00s

test fixtures::pinned_fixtures::environment_tests::ops_root_uses_the_contract ... ok

test result: ok
test result: . 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered outok. 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out; finished in 0.00s

; finished in 0.00s

test fixtures::pinned_fixtures::environment_tests::absent_fallback_is_optional ... ok
test fixtures::pinned_fixtures::environment_tests::ops_root_uses_the_contract ... ok
test reads_every_mac_new_header_field ... ignored, requires the pinned ExifTool fixture cache

test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.21s

"""

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
        self.seen = []; self.suite_calls = []; self.oracle_calls = []; self.diff_output = ".exiftool-version\n"
        (self.native / "exiftool").write_text("#!/usr/bin/env perl\n")
        (self.native / "t/images").mkdir(parents=True); (self.native / "t/images/OOXML.docx").write_bytes(b"PK")
        self.oracle_version, self.oracle_docx, self.missing_module = "11.78", "DOCX", None
        (self.checkout / "tools/release").mkdir(parents=True)
        (self.checkout / "tools/release/bootstrap_oracle.py").write_text("# bootstrap placeholder\n")
        self.ops = self.root / "ops"
        self.corpus = self.ops / "cache/exiftool/13.59/combined-samples"
        (self.corpus / "Apple").mkdir(parents=True)
        (self.corpus / "ExifTool.jpg").write_bytes(b"\xff\xd8exiftool")
        (self.corpus / "Apple/iPhone.jpg").write_bytes(b"\xff\xd8apple")
        self.storage = self.ops / "evidence/storage-manifest.json"
        self.bootstrap = SimpleNamespace(
            VERSION="13.59", LOCK={"corpus_tree_sha256": "7" * 64},
            corpus_path=lambda root: root / "cache/exiftool/13.59/combined-samples",
            manifest_path=lambda root: root / "evidence/storage-manifest.json")
        self.verify_calls, self.verify_exit, self.corpus_during_run = [], 0, None
        for name, value in (("_ops_root", lambda: self.ops), ("_bootstrap_module", lambda _checkout: self.bootstrap)):
            patcher = patch.object(adapter, name, side_effect=value, create=True)
            patcher.start(); self.addCleanup(patcher.stop)
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
        if argv == ["rustc", "-vV"]:
            return subprocess.CompletedProcess(argv, 0, "rustc 1.97.1 (fixture 2026-01-01)\nhost: aarch64-apple-darwin\n", "")
        if argv == ["cargo", "-V"]:
            return subprocess.CompletedProcess(argv, 0, "cargo 1.97.1 (fixture 2026-01-01)\n", "")
        if argv[:2] == ["cargo", "test"] and "--no-run" not in argv:
            self.suite_calls.append((argv, kwargs["env"]))
            if kwargs.get("stderr") is not subprocess.STDOUT:
                raise AssertionError("release tests must merge stderr into stdout")
            if self.corpus_during_run is not None:
                self.corpus_during_run()
            return subprocess.CompletedProcess(argv, 0, SUITE_TESTS_OUTPUT, None)
        if Path(argv[0]).name == "perl":
            self.oracle_calls.append((argv, kwargs["env"]))
            if argv[1].startswith("-M"):
                code = 1 if argv[1][2:] == self.missing_module else 0
                return subprocess.CompletedProcess(argv, code, "", "")
            if argv[-1] == "-ver":
                return subprocess.CompletedProcess(argv, 0, self.oracle_version + "\n", None)
            if argv[-3:-1] == ["-s3", "-FileType"]:
                return subprocess.CompletedProcess(argv, 0, self.oracle_docx + "\n", None)
        if argv[0] == "cargo":
            test = argv[1] == "test"
            binary = self.target / ("debug/deps/oxidex-writer-test" if test else "debug/oxidex")
            binary.parent.mkdir(parents=True, exist_ok=True); binary.write_bytes(b"writer" if test else b"binary"); binary.chmod(0o755)
            return subprocess.CompletedProcess(argv, 0, json.dumps({"reason": "compiler-artifact", "manifest_path": str(self.checkout / "Cargo.toml"), "profile": {"test": test}, "target": {"name": "oxidex", "kind": ["lib"] if test else ["bin"]}, "executable": str(binary)}) + "\n", "")
        if argv[0] == sys.executable and argv[1].endswith("tools/release/bootstrap_oracle.py"):
            self.verify_calls.append((argv, kwargs["env"]))
            if self.verify_exit:
                return subprocess.CompletedProcess(argv, self.verify_exit, "refused: corpus lock hash mismatch", None)
            root = Path(argv[argv.index("--root") + 1])
            manifest = root / "cache/exiftool/13.59/combined-samples.manifest"
            manifest.write_text("".join(
                f"{adapter._sha(item)}  {item.relative_to(self.corpus).as_posix()}\n"
                for item in sorted((value for value in self.corpus.rglob("*") if value.is_file()),
                                   key=lambda value: value.relative_to(self.corpus).as_posix())))
            self.storage.parent.mkdir(parents=True, exist_ok=True)
            self.storage.write_text(json.dumps({"artifacts": {
                "corpus_manifest": {"kind": "file", "path": str(manifest), "sha256": adapter._sha(manifest)},
                "corpus_tree": {"kind": "tree", "path": str(self.corpus), "sha256": "7" * 64}}}))
            return subprocess.CompletedProcess(argv, 0, str(self.storage), None)
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

    def test_release_test_suite_runs_in_isolated_target_and_counts_strictly(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        built = adapter.build(self.args("build"), run=self.fake_run)
        result = adapter.run_release_tests(self.args("test"), run=self.fake_run)
        self.assertEqual(result["state"], "passed")
        # One invocation, like CI's required step, but over the whole workspace.
        self.assertEqual(adapter.TEST_COMMANDS, (("cargo", "test", "--workspace", "--all-features", "--no-fail-fast"),))
        self.assertEqual(result["test_suite"]["scope"], adapter.TEST_SCOPE)
        self.assertIn("superset", adapter.TEST_SCOPE)
        self.assertIn("cargo test --all-features", adapter.TEST_SCOPE)
        self.assertEqual([argv for argv, _env in self.suite_calls], [list(row) for row in adapter.TEST_COMMANDS])
        suite_target = self.target.resolve() / "test-suite"
        self.assertTrue(all(env["CARGO_TARGET_DIR"] == str(suite_target) and env["CARGO_TERM_COLOR"] == "never"
                            for _argv, env in self.suite_calls))
        suite = result["test_suite"]
        self.assertEqual(suite["totals"], {"passed": 5, "failed": 0, "ignored": 1, "measured": 0,
                                           "filtered_out": 0, "targets": 4})
        self.assertEqual(result["denominator"], 5)
        self.assertEqual([row["exit"] for row in suite["commands"]], [0])
        self.assertTrue(all(type(row["duration_seconds"]) is float for row in suite["commands"]))
        self.assertEqual(suite["target_directory"], str(suite_target))
        self.assertEqual(suite["log"], result["raw_report"])
        log = json.loads(Path(suite["log"]["path"]).read_text())
        self.assertEqual([row["stdout"] for row in log["commands"]], [SUITE_TESTS_OUTPUT])
        # The suite's own target keeps the build's CLI and writer driver intact.
        adapter.executor._require_binary_proof(built, self.target)
        adapter.executor._require_binary_proof(built, self.target, "writer_binary")
        with self.assertRaisesRegex(adapter.Refused, "stale reuse"):
            adapter.run_release_tests(self.args("test", report=str(self.reports / "again" / "test.json")),
                                      run=self.fake_run)

    def test_release_tests_use_the_selected_oracle_and_ignore_ambient_overrides(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)
        ambient = {
            "EXIFTOOL": "/opt/homebrew/bin/exiftool", "EXIFTOOL_CACHE_DIR": "/tmp/foreign-cache",
            "EXIFTOOL_PERL": "/opt/homebrew/bin/perl", "OXIDEX_ALLOW_EXIFTOOL_SKEW": "1",
            "OXIDEX_PINNED_EXIFTOOL": "/tmp/other", "CARGO_TERM_QUIET": "true", "RUSTFLAGS": "--cfg skip",
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER": "true", "RUSTC_WRAPPER": "true",
            "OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES": "0",
        }
        with patch.dict(os.environ, ambient):
            result = adapter.run_release_tests(self.args("test"), run=self.fake_run)
        perl, native = self.perl.resolve(), self.native.resolve()
        suite_target = self.target.resolve() / "test-suite"
        cache = suite_target / "exiftool-oracle"
        for env in [env for _argv, env in self.suite_calls] + [env for _argv, env in self.oracle_calls]:
            self.assertFalse({"EXIFTOOL", "OXIDEX_ALLOW_EXIFTOOL_SKEW", "OXIDEX_PINNED_EXIFTOOL",
                              "CARGO_TERM_QUIET", "RUSTFLAGS", "RUSTC_WRAPPER",
                              "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER"} & set(env))
            self.assertLessEqual(set(env), set(adapter.TEST_ENVIRONMENT_PASSTHROUGH) | set(adapter.TEST_ENVIRONMENT_SET))
            self.assertEqual(env["EXIFTOOL_CACHE_DIR"], str(cache))
            self.assertEqual(env["EXIFTOOL_PERL"], str(perl))
            self.assertEqual(env["OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES"], "1")
            self.assertTrue(env["PATH"].startswith(str(cache / "bin") + os.pathsep))
        self.assertEqual((cache / "exiftool").resolve(), native)
        shim = (cache / "bin" / "exiftool").read_text()
        self.assertIn(str(perl), shim); self.assertIn(str(cache / "exiftool" / "exiftool"), shim)
        oracle = result["test_suite"]["exiftool_oracle"]
        self.assertEqual(oracle["version"], "11.78")
        self.assertEqual(oracle["docx_filetype"], "DOCX")
        self.assertTrue(oracle["perl_modules_available"])
        self.assertEqual(oracle["tree_realpath"], str(native))
        self.assertEqual(oracle["cache_dir"], str(cache))
        self.assertEqual(oracle["perl"]["path"], str(perl))
        self.assertEqual(oracle["lib"]["exiftool_pm_sha256"], result["native_identity"]["lib"]["exiftool_pm_sha256"])
        self.assertEqual(result["test_suite"]["environment"], self.suite_calls[0][1])
        # The probes run exactly the argv the Rust oracle builds from EXIFTOOL_CACHE_DIR.
        self.assertIn([str(perl), f"-I{cache / 'exiftool' / 'lib'}", str(cache / "exiftool" / "exiftool"), "-ver"],
                      [argv for argv, _env in self.oracle_calls])

    def test_release_tests_bind_the_verified_combined_corpus(self):
        self.maxDiff = None
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)
        result = adapter.run_release_tests(self.args("test"), run=self.fake_run)
        self.assertEqual(result["state"], "passed")
        argv, env = self.verify_calls[0]
        self.assertEqual(argv, [sys.executable, str(self.checkout.resolve() / "tools/release/bootstrap_oracle.py"),
                                "verify", "--root", str(self.ops), "--pin", "13.59"])
        self.assertEqual(env["OXIDEX_OPS_DIR"], str(self.ops))
        order = [row[0] for row in self.seen]
        self.assertLess(order.index(argv), order.index(list(adapter.TEST_COMMANDS[0])))
        cache = self.target.resolve() / "test-suite" / "exiftool-oracle"
        self.assertEqual((cache / "combined-samples").resolve(), self.corpus.resolve())
        manifest = self.ops / "cache/exiftool/13.59/combined-samples.manifest"
        self.assertEqual(result["test_suite"]["fixture_corpus"], {
            "ops_root": str(self.ops), "bootstrap_pin": "13.59", "version_independent": True,
            "corpus": str(self.corpus), "link": str(cache / "combined-samples"),
            "corpus_tree_sha256": "7" * 64,
            "manifest": {"path": str(manifest), "sha256": adapter._sha(manifest), "file_count": 2},
            "storage_manifest": {"path": str(self.storage), "sha256": adapter._sha(self.storage)},
            "verify_command": argv, "verified_before_run": True, "verified_after_run": True,
        })

    def test_release_tests_refuse_an_unverified_or_drifted_corpus(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)

        def verify_then(mutate):
            def run(argv, **kwargs):
                result = self.fake_run(argv, **kwargs)
                if len(argv) > 1 and argv[1].endswith("bootstrap_oracle.py"):
                    mutate()
                return result
            return run

        def rewrite(path, data):
            return lambda: path.write_bytes(data)

        def storage_edit(old, new):
            return lambda: self.storage.write_text(self.storage.read_text().replace(old, new))

        manifest = self.ops / "cache/exiftool/13.59/combined-samples.manifest"
        cases = {
            "verify refused": ("exit", None),
            "storage manifest differs": ("after-verify", lambda: storage_edit(adapter._sha(manifest), "0" * 64)()),
            "wrong lock tree": ("after-verify", storage_edit("7" * 64, "8" * 64)),
            "manifest rewritten": ("after-verify", lambda: manifest.write_text(
                manifest.read_text().replace("Apple/iPhone.jpg", "Apple/other.jpg"))),
            "sample changed after verify": ("after-verify", rewrite(self.corpus / "ExifTool.jpg", b"changed")),
            "unlisted sample": ("after-verify", rewrite(self.corpus / "Apple/extra.jpg", b"extra")),
            "sample removed": ("after-verify", lambda: (self.corpus / "Apple/iPhone.jpg").unlink()),
            "sample changed during run": ("during-run", rewrite(self.corpus / "ExifTool.jpg", b"tests wrote")),
        }
        for label, (when, mutate) in cases.items():
            with self.subTest(label=label):
                shutil.rmtree(self.corpus); (self.corpus / "Apple").mkdir(parents=True)
                (self.corpus / "ExifTool.jpg").write_bytes(b"\xff\xd8exiftool")
                (self.corpus / "Apple/iPhone.jpg").write_bytes(b"\xff\xd8apple")
                report = self.reports / label.replace(" ", "-") / "test.json"
                report.parent.mkdir()
                (report.parent / "build.json").write_text((self.reports / "build.json").read_text())
                shutil.rmtree(self.target / "test-suite", ignore_errors=True)
                self.suite_calls.clear()
                self.verify_exit, self.corpus_during_run = (1 if when == "exit" else 0), None
                run = self.fake_run
                if when == "after-verify":
                    run = verify_then(mutate)
                elif when == "during-run":
                    self.corpus_during_run = mutate
                with self.assertRaisesRegex(adapter.Refused, "fixture corpus"):
                    adapter.run_release_tests(self.args("test", report=str(report)), run=run)
                self.assertFalse(report.exists())
                if when != "during-run":
                    self.assertEqual(self.suite_calls, [])
        self.verify_exit, self.corpus_during_run = 0, None

    def test_release_tests_refuse_cargo_configuration_outside_the_checkout(self):
        (self.checkout / ".cargo").mkdir(); (self.checkout / ".cargo/config.toml").write_text("[alias]\n")
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)
        cargo_home = self.root / "cargo-home"; cargo_home.mkdir()
        cases = {
            "parent config.toml": (self.root / ".cargo/config.toml", {}),
            "parent legacy config": (self.root / ".cargo/config", {}),
            "CARGO_HOME config": (cargo_home / "config.toml", {"CARGO_HOME": str(cargo_home)}),
        }
        for label, (config, env) in cases.items():
            with self.subTest(label=label):
                config.parent.mkdir(exist_ok=True); config.write_text("[build]\nrustflags = ['--cfg', 'skip']\n")
                report = self.reports / label.replace(" ", "-") / "test.json"
                report.parent.mkdir()
                (report.parent / "build.json").write_text((self.reports / "build.json").read_text())
                shutil.rmtree(self.target / "test-suite", ignore_errors=True)
                self.suite_calls.clear()
                with patch.dict(os.environ, env):
                    with self.assertRaisesRegex(adapter.Refused, "cargo configuration outside the checkout"):
                        adapter.run_release_tests(self.args("test", report=str(report)), run=self.fake_run)
                self.assertEqual(self.suite_calls, [])
                config.unlink()
        # The checkout's own tracked .cargo/config.toml is source, not ambient configuration.
        shutil.rmtree(self.target / "test-suite", ignore_errors=True)
        clean = self.reports / "clean"; clean.mkdir()
        (clean / "build.json").write_text((self.reports / "build.json").read_text())
        result = adapter.run_release_tests(self.args("test", report=str(clean / "test.json")), run=self.fake_run)
        self.assertEqual(result["test_suite"]["cargo_config"]["outside_checkout"], [])
        self.assertIn(str(self.root.resolve() / ".cargo" / "config.toml"),
                      result["test_suite"]["cargo_config"]["checked"])

    def test_release_test_oracle_must_be_the_selected_capable_release(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)
        for label, change in (("wrong release", {"oracle_version": "13.55"}),
                              ("zip degraded", {"oracle_docx": "ZIP"}),
                              ("module missing", {"missing_module": "Archive::Zip"})):
            with self.subTest(label=label):
                report = self.reports / label.replace(" ", "-") / "test.json"
                report.parent.mkdir()
                (report.parent / "build.json").write_text((self.reports / "build.json").read_text())
                shutil.rmtree(self.target / "test-suite", ignore_errors=True)
                self.oracle_version, self.oracle_docx, self.missing_module = "11.78", "DOCX", None
                for name, value in change.items():
                    setattr(self, name, value)
                self.suite_calls.clear()
                with self.assertRaisesRegex(adapter.Refused, "release test oracle"):
                    adapter.run_release_tests(self.args("test", report=str(report)), run=self.fake_run)
                self.assertEqual(self.suite_calls, [])
                self.assertFalse(report.exists())

    def test_release_test_failures_publish_a_failed_report_not_a_pass(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)
        def failing(argv, **kwargs):
            result = self.fake_run(argv, **kwargs)
            if argv[:2] == ["cargo", "test"] and "--no-run" not in argv:
                return subprocess.CompletedProcess(argv, 101, SUITE_FAILED_OUTPUT, None)
            return result
        result = adapter.run_release_tests(self.args("test"), run=failing)
        self.assertEqual(result["state"], "failed")
        self.assertEqual(result["test_suite"]["totals"]["failed"], 1)
        self.assertEqual(result["test_suite"]["commands"][0]["exit"], 101)
        with self.assertRaisesRegex(adapter.executor.Refused, "passed state"):
            adapter.executor._stage_result(self.reports / "test.json", "11.78", "test", None)

    def test_unparsable_or_incomplete_test_output_refuses(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        adapter.build(self.args("build"), run=self.fake_run)
        nested_crash = SUITE_NESTED_OUTPUT.rsplit("test result: ok. 3 passed", 1)[0]
        doc_heading = "   Doc-tests tiny\n"
        broken = {
            "garbled result": (0, SUITE_TESTS_OUTPUT.replace("0 measured; ", "", 1)),
            "crashed target": (101, SUITE_TESTS_OUTPUT.split("   Doc-tests", 1)[0].rsplit("test result:", 1)[0]),
            "count mismatch": (0, SUITE_TESTS_OUTPUT.replace("running 3 tests", "running 4 tests")),
            "exit disagrees": (101, SUITE_TESTS_OUTPUT),
            "status disagrees": (0, SUITE_TESTS_OUTPUT.replace("test result: ok. 2 passed; 0 failed",
                                                               "test result: ok. 1 passed; 1 failed")),
            "nothing ran": (101, "error[E0599]: no variant named `ValBpm`\n"),
            "nested summary only": (101, nested_crash),
            "filtered outer run": (0, SUITE_TESTS_OUTPUT.replace("0 measured; 0 filtered out", "0 measured; 2 filtered out", 1)),
            "doc block without summary": (0, SUITE_TESTS_OUTPUT.replace(doc_heading, doc_heading + "\nrunning 2 tests\n")),
            "garbled doc summary": (0, SUITE_TESTS_OUTPUT.rsplit("test result:", 1)[0] + "test result: ok\n"),
        }
        for label, (code, stdout) in broken.items():
            with self.subTest(label=label):
                report = self.reports / label.replace(" ", "-") / "test.json"
                report.parent.mkdir()
                (report.parent / "build.json").write_text((self.reports / "build.json").read_text())
                shutil.rmtree(self.target / "test-suite", ignore_errors=True)
                def output(argv, **kwargs):
                    if argv[:2] == ["cargo", "test"] and "--no-run" not in argv:
                        return subprocess.CompletedProcess(argv, code, stdout, None)
                    return self.fake_run(argv, **kwargs)
                with self.assertRaisesRegex(adapter.Refused, "cargo test"):
                    adapter.run_release_tests(self.args("test", report=str(report)), run=output)
                self.assertFalse(report.exists())

    def test_nested_self_reexec_output_counts_only_each_targets_own_run(self):
        self.assertEqual(adapter.parse_test_output(SUITE_NESTED_OUTPUT), {
            "passed": 3, "failed": 0, "ignored": 1, "measured": 0, "filtered_out": 0, "targets": 1,
        })
        self.assertEqual(adapter.parse_test_output(SUITE_TESTS_OUTPUT)["targets"], 4)

    def test_edition_2024_merged_and_standalone_doctest_blocks_are_summed(self):
        self.assertEqual(adapter.parse_test_output(SUITE_EDITION2024_OUTPUT), {
            "passed": 6, "failed": 0, "ignored": 0, "measured": 0, "filtered_out": 0, "targets": 3,
        })
        broken = SUITE_EDITION2024_OUTPUT.replace("running 1 test\ntest src/lib.rs - one (line 9)",
                                                  "running 2 tests\ntest src/lib.rs - one (line 9)")
        with self.assertRaisesRegex(adapter.Refused, "cargo test"):
            adapter.parse_test_output(broken)

    def test_release_test_suite_requires_the_passed_build(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        with self.assertRaises((adapter.Refused, OSError)):
            adapter.run_release_tests(self.args("test"), run=self.fake_run)
        self.assertEqual(self.suite_calls, [])

    def test_build_uses_the_allowlisted_environment_and_records_its_toolchain(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        hostile = {
            "RUSTFLAGS": "--cfg forged", "CARGO_ENCODED_RUSTFLAGS": "--cfg\x1fforged", "RUSTC": "/tmp/rustc",
            "RUSTC_WRAPPER": "/tmp/wrap", "RUSTC_WORKSPACE_WRAPPER": "/tmp/wrap", "CARGO_BUILD_TARGET": "x86_64",
            "CARGO_BUILD_RUSTFLAGS": "--cfg forged", "CARGO_TARGET_DIR": "/tmp/elsewhere", "RUSTDOCFLAGS": "-x",
            "CARGO_PROFILE_DEV_OPT_LEVEL": "3", "EXIFTOOL": "/opt/homebrew/bin/exiftool",
        }
        first = len(self.seen)
        with patch.dict(os.environ, hostile):
            built = adapter.build(self.args("build"), run=self.fake_run)
        calls = [(argv, env) for argv, env in self.seen[first:] if argv[0] in {"cargo", "rustc"}]
        self.assertEqual([argv for argv, _env in calls][:2], [["rustc", "-vV"], ["cargo", "-V"]])
        allowed = set(adapter.BUILD_ENVIRONMENT_PASSTHROUGH) | set(adapter.BUILD_ENVIRONMENT_SET)
        for argv, env in calls:
            self.assertFalse((set(hostile) - {"CARGO_TARGET_DIR"}) & set(env), argv)
            self.assertLessEqual(set(env), allowed, argv)
            self.assertEqual(env["CARGO_TARGET_DIR"], str(self.target.resolve()))
        build_env = built["build_environment"]
        self.assertEqual(build_env["environment"], calls[-1][1])
        self.assertEqual(build_env["toolchain"], {
            "rustc": "rustc 1.97.1 (fixture 2026-01-01)\nhost: aarch64-apple-darwin",
            "cargo": "cargo 1.97.1 (fixture 2026-01-01)"})
        self.assertEqual(build_env["cargo_config"]["outside_checkout"], [])

    def test_build_refuses_cargo_configuration_outside_the_checkout(self):
        adapter.generate(self.args("generate"), run=self.fake_run)
        config = self.root / ".cargo" / "config.toml"
        config.parent.mkdir(); config.write_text("[build]\nrustflags = ['--cfg', 'forged']\n")
        first = len(self.seen)
        with self.assertRaisesRegex(adapter.Refused, "cargo configuration outside the checkout"):
            adapter.build(self.args("build"), run=self.fake_run)
        self.assertFalse(any(argv[:2] in (["cargo", "build"], ["cargo", "test"]) for argv, _env in self.seen[first:]))
        self.assertFalse((self.reports / "build.json").exists())

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
            if argv[0] == "cargo" and argv[1] in {"build", "test"}:
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


    def test_canonical_unsupported_populations_are_counted_once(self):
        """sanitize/convinv ledgers declare top-level unsupported_branches; count each once."""
        with TemporaryDirectory() as temporary:
            checkout = Path(temporary)
            (checkout / "sanitize.json").write_text(json.dumps({
                "omissions": [],
                "unsupported_branches": [f"branch-{index}" for index in range(6)],
                "primitive_contract": {"final": {"unsupported_domains": ["a", "b", "c"]},
                                       "pristine": {"unsupported_domains": ["a", "b", "c"]}},
            }))
            (checkout / "convinv.json").write_text(json.dumps({
                "unsupported_branches": [f"branch-{index}" for index in range(5)],
            }))
            (checkout / "rows.json").write_text(json.dumps({
                "rows_omitted": 9,
                "omissions_by_reason": {"row has unsupported source properties: Shift": 7,
                                        "CHECK_PROC: CHECK_PROC format selector set is unsupported": 2},
            }))
            inventory = [SimpleNamespace(path=name) for name in ("sanitize.json", "convinv.json", "rows.json")]
            with patch.object(adapter.artifacts, "inventory", return_value=inventory):
                result = adapter.generated_refusal_counts(checkout)
            self.assertEqual(
                {(row["artifact"], row["json_path"], row["count"]) for row in result["counters"]},
                {("sanitize.json", "unsupported_branches", 6), ("convinv.json", "unsupported_branches", 5),
                 ("rows.json", "rows_omitted", 9)},
            )
            self.assertEqual(result["total"], 20)

    def test_real_unsupported_ledgers_are_counted(self):
        real = adapter.generated_refusal_counts(HERE.parents[1])
        counted = {(row["artifact"], row["json_path"]): row["count"] for row in real["counters"]}
        for name in ("sanitize_ledger.json", "convinv_ledger.json"):
            path = HERE / name
            branches = json.loads(path.read_text())["unsupported_branches"]
            self.assertEqual(counted[(f"tools/exiftool-tables/{name}", "unsupported_branches")], len(branches))
        self.assertFalse(any("unsupported_domains" in path or "omissions_by_reason" in path
                             for _artifact, path in counted))

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
