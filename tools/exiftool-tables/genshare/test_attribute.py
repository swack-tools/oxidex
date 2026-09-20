"""Contract tests for the authenticated generated-route attribution receipt."""

import copy
import hashlib
import importlib.util
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[3]
SCRATCH = ROOT / ".superpowers/sdd/task8-receipt-repair-design/test-tmp"
SCRATCH.mkdir(parents=True, exist_ok=True)
MODULE = ROOT / "tools/exiftool-tables/genshare/attribute.py"
SPEC = importlib.util.spec_from_file_location("genshare_attribute", MODULE)
attribute = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(attribute)


def canonical_sha(value):
    data = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(data.encode()).hexdigest()


class TokenContractTests(unittest.TestCase):
    def test_exact_six_token_order_and_independent_union_are_required(self):
        self.assertEqual(
            attribute.TOKENS,
            ("engine", "legacy-l1", "legacy-l2", "producers", "serial", "keyed"),
        )
        contract = attribute.validate_token_request(",".join(attribute.TOKENS))
        self.assertEqual(contract["individual"], list(attribute.TOKENS))
        self.assertEqual(contract["union"], ",".join(attribute.TOKENS))

    def test_duplicate_empty_unknown_and_conv_exit_two(self):
        bad = [
            "engine,engine,legacy-l1,legacy-l2,producers,serial,keyed",
            "engine,legacy-l1,legacy-l2,producers,serial,keyed,",
            "engine,legacy-l1,legacy-l2,producers,serial,unknown",
            "engine,legacy-l1,legacy-l2,producers,serial,conv",
            "legacy-l1,engine,legacy-l2,producers,serial,keyed",
        ]
        for raw in bad:
            with self.subTest(raw=raw), self.assertRaises(attribute.ReceiptError):
                attribute.validate_token_request(raw)


class ParsingAndProjectionTests(unittest.TestCase):
    def test_duplicate_json_keys_fail_closed(self):
        with self.assertRaisesRegex(attribute.ReceiptError, "duplicate JSON key"):
            attribute.parse_json_output(b'[{"ICC:Header":1,"ICC:Header":2}]', "candidate")

    def test_one_missing_pair_is_one_occurrence_not_two(self):
        oracle = {"File:FileType": "ICC", "ICC_Profile:Header": "abc"}
        control = {"File:FileType": "ICC", "ICC-header:Header": "abc"}
        probe = {"File:FileType": "ICC"}
        control_projection = attribute.project_file(oracle, control)
        probe_projection = attribute.project_file(oracle, probe)
        reconciliation = attribute.reconcile(control_projection, probe_projection)
        self.assertEqual(probe_projection["missing_occurrences"], 1)
        self.assertEqual(reconciliation["matched_lost"], 1)
        self.assertEqual(reconciliation["oracle_rebuild"], 1)
        self.assertEqual(reconciliation["oracle_residual"], 0)
        self.assertEqual(reconciliation["candidate_residual"], 0)

    def test_zero_delta_is_unexercised_not_authenticated(self):
        counts = {
            "oracle_occurrences": 1,
            "candidate_occurrences": 1,
            "matched_occurrences": 1,
            "missing_occurrences": 0,
            "extra_occurrences": 0,
            "value_occurrences": 0,
            "rename_source_occurrences": 0,
            "rename_target_occurrences": 0,
        }
        self.assertEqual(attribute.exercise_status(counts, counts, production_reachable=True), "unexercised")
        self.assertEqual(attribute.exercise_status(counts, counts, production_reachable=False), "unexercised")

    def test_only_exact_system_file_access_date_value_is_normalized(self):
        candidate = {
            "System:FileAccessDate": "2026:09:20 01:02:03-07:00",
            "File:FileAccessDate": "must-not-change",
            "System:FileInodeChangeDate": "must-not-change",
            "XMP:Value": 1,
        }
        normalized = attribute.occurrence_sequence(candidate, normalize_access_date=True)
        self.assertEqual(normalized[0]["value"], "<NORMALIZED:FileAccessDate>")
        self.assertEqual(
            normalized[0]["raw_serialized"], '"<NORMALIZED:FileAccessDate>"'
        )
        self.assertEqual(normalized[0]["raw_key"], "System:FileAccessDate")
        self.assertEqual(normalized[0]["position"], 0)
        self.assertEqual(normalized[1]["value"], "must-not-change")
        self.assertEqual(normalized[2]["value"], "must-not-change")

    def test_stable_output_normalizes_only_each_sides_exact_access_date_key(self):
        oracle = {
            "File:System:FileAccessDate": "volatile",
            "System:FileAccessDate": "must-not-change",
            "File:System:FileModifyDate": "must-not-change",
        }
        candidate = {
            "System:FileAccessDate": "volatile",
            "File:System:FileAccessDate": "must-not-change",
            "System:FileModifyDate": "must-not-change",
        }

        stable_oracle = attribute.normalize_access_date_output(oracle, oracle=True)
        stable_candidate = attribute.normalize_access_date_output(candidate, oracle=False)

        self.assertEqual(
            stable_oracle["File:System:FileAccessDate"],
            "<NORMALIZED:FileAccessDate>",
        )
        self.assertEqual(stable_oracle["System:FileAccessDate"], "must-not-change")
        self.assertEqual(
            stable_candidate["System:FileAccessDate"],
            "<NORMALIZED:FileAccessDate>",
        )
        self.assertEqual(
            stable_candidate["File:System:FileAccessDate"], "must-not-change"
        )

    def test_each_child_retains_rc_stdout_stderr_and_parsed_output(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            child = pathlib.Path(td)
            record = attribute.capture_process(
                [
                    sys.executable,
                    "-c",
                    "import sys; print('[{\\\"File:FileType\\\":\\\"ICC\\\"}]'); "
                    "print('diagnostic', file=sys.stderr)",
                ],
                pathlib.Path(td),
                child,
                "candidate",
                {"OXIDEX_GENSHARE_SILENCE": {"state": "empty", "value": ""}},
            )
            self.assertEqual(record["returncode"], 0)
            self.assertEqual(record["parse_status"], "ok")
            self.assertEqual(record["parsed"]["sha256"], hashlib.sha256(
                (child / "candidate.parsed.json").read_bytes()
            ).hexdigest())
            self.assertIn(b"diagnostic", (child / "candidate.stderr").read_bytes())
            self.assertEqual(record["argv"][0], sys.executable)
            self.assertEqual(record["process_identity"]["pid"], record["pid"])
            self.assertEqual(
                record["process_identity"]["captured"],
                record["process_identity"]["verified_before_communicate"],
            )

    def test_child_refuses_pid_identity_change_before_communicate(self):
        identities = [
            {"platform": "linux", "boot_id": "boot", "start_token": "100"},
            {"platform": "linux", "boot_id": "boot", "start_token": "101"},
        ]
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td, mock.patch.object(
            attribute, "_kernel_process_identity", side_effect=identities, create=True
        ):
            with self.assertRaisesRegex(attribute.ChildProcessError, "identity changed"):
                attribute.capture_process(
                    [sys.executable, "-c", "print('[{\"File:FileType\":\"ICC\"}]')"],
                    pathlib.Path(td),
                    pathlib.Path(td),
                    "candidate",
                    {},
                )

    def test_nonzero_child_is_retained_and_fails_closed(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            child = pathlib.Path(td)
            with self.assertRaisesRegex(attribute.ChildProcessError, "return code 7"):
                attribute.capture_process(
                    [sys.executable, "-c", "import sys; print('bad', file=sys.stderr); sys.exit(7)"],
                    pathlib.Path(td),
                    child,
                    "oracle",
                    {},
                )
            self.assertEqual((child / "oracle.returncode").read_text(), "7\n")
            self.assertIn("bad", (child / "oracle.stderr").read_text())


class ArtifactValidationTests(unittest.TestCase):
    def test_validator_recomputes_artifact_hashes_and_token_set(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            run_root = pathlib.Path(td)
            artifact = run_root / "evidence.json"
            artifact.write_text('{"ok":true}\n', encoding="utf-8")
            st = artifact.stat()
            receipt = {
                "schema": "genshare-receipt/v3",
                "status": "observed_unreviewed",
                "run_root": str(run_root.resolve()),
                "token_contract": {
                    "individual": list(attribute.TOKENS),
                    "union": ",".join(attribute.TOKENS),
                },
                "artifact_index": [{
                    "relative_path": "evidence.json",
                    "size": st.st_size,
                    "mode": st.st_mode & 0o777,
                    "sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                }],
                "projections": {},
                "reconciliations": {},
                "pre_seam": None,
                "pre_seam_control": None,
                "failed_stage": None,
                "failure": None,
            }
            attribute.validate_v3_receipt(receipt, run_root, replay=False)
            mutated = copy.deepcopy(receipt)
            mutated["artifact_index"][0]["sha256"] = "0" * 64
            with self.assertRaisesRegex(attribute.ReceiptError, "artifact"):
                attribute.validate_v3_receipt(mutated, run_root, replay=False)
            mutated = copy.deepcopy(receipt)
            mutated["token_contract"]["individual"][-1] = "conv"
            with self.assertRaisesRegex(attribute.ReceiptError, "token"):
                attribute.validate_v3_receipt(mutated, run_root, replay=False)

    def test_observed_and_failed_receipts_never_pass_require_success(self):
        for status in ("observed_unreviewed", "failed"):
            with self.subTest(status=status), self.assertRaisesRegex(attribute.ReceiptError, "success"):
                attribute.require_success_status(status)

    def test_path_set_hash_is_ordered_and_exact(self):
        rows = ["ICC_Profile.icc", "AAC.aac", "OOXML.docx"]
        self.assertEqual(attribute.path_set_sha256(rows), canonical_sha(rows))
        self.assertNotEqual(attribute.path_set_sha256(rows), attribute.path_set_sha256(list(reversed(rows))))

    def test_validator_replays_raw_children_and_rejects_mutated_counter(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            root = pathlib.Path(td)
            relative_paths = ["ICC_Profile.icc", "AAC.aac", "OOXML.docx"]
            selection_root = root / "selection"
            selection_root.mkdir()
            manifest_rows = []
            for relative in relative_paths:
                staged = selection_root / relative
                staged.write_bytes(relative.encode())
                manifest_rows.append({
                    "relative_path": relative,
                    "source_path": str(staged.resolve()),
                    "staged_path": str(staged.resolve()),
                    "size": staged.stat().st_size,
                    "mode": staged.stat().st_mode & 0o777,
                    "sha256": hashlib.sha256(staged.read_bytes()).hexdigest(),
                })
            runs = {}
            projections = {}
            for mode in (
                "pre-seam-control", "control-unset", "control-empty", *attribute.TOKENS, "union"
            ):
                oracle_document = {
                    "File:System:FileAccessDate": f"oracle-{mode}",
                    "File:FileType": "ICC",
                    "File:FileTypeExtension": "icc",
                    "File:MIMEType": "application/vnd.iccprofile",
                    "ICC_Profile:Header": "abc",
                }
                oracle_json = json.dumps([oracle_document])
                full_document = {
                    "System:FileAccessDate": f"candidate-{mode}",
                    "File:FileType": "ICC",
                    "File:FileTypeExtension": "icc",
                    "File:MIMEType": "application/vnd.iccprofile",
                    "ICC-header:Header": "abc",
                }
                children = []
                per_file = {}
                for number, relative in enumerate(relative_paths, 1):
                    child = root / "runs" / mode / "children" / f"{number:06d}"
                    oracle = attribute.capture_process(
                        [sys.executable, "-c", f"print({oracle_json!r})"],
                        root,
                        child,
                        "oracle",
                        {},
                    )
                    candidate_document = dict(full_document)
                    if mode in ("producers", "union"):
                        for key in (
                            "File:FileType", "File:FileTypeExtension", "File:MIMEType"
                        ):
                            candidate_document.pop(key)
                    if mode in ("engine", "union") and relative == "ICC_Profile.icc":
                        candidate_document.pop("ICC-header:Header")
                    candidate_text = json.dumps([candidate_document])
                    candidate = attribute.capture_process(
                        [sys.executable, "-c", f"print({candidate_text!r})"],
                        root,
                        child,
                        "candidate",
                        {"OXIDEX_GENSHARE_SILENCE": {"state": "value", "value": mode}},
                    )
                    candidate_parsed = attribute.parse_json_output(candidate_text.encode(), "candidate")
                    process_path = child / "process.json"
                    process_path.write_text(json.dumps({
                        "oracle": oracle,
                        "candidate": candidate,
                        "candidate_occurrences": attribute.occurrence_sequence(
                            candidate_parsed, normalize_access_date=True
                        ),
                    }) + "\n")
                    child_record = {
                        "relative_path": relative,
                        "process": {"path": str(process_path.resolve()), **{
                            "size": process_path.stat().st_size,
                            "mode": process_path.stat().st_mode & 0o777,
                            "sha256": hashlib.sha256(process_path.read_bytes()).hexdigest(),
                        }},
                    }
                    children.append(child_record)
                    oracle_parsed = attribute.parse_json_output(oracle_json.encode(), "oracle")
                    per_file[relative] = attribute.project_file(
                        attribute.normalize_access_date_output(
                            oracle_parsed, oracle=True
                        ),
                        attribute.normalize_access_date_output(
                            candidate_parsed, oracle=False
                        ),
                    )
                runs[mode] = {
                    "path_set_sha256": attribute.path_set_sha256(relative_paths),
                    "children": children,
                }
                aggregate = {key: sum(row[key] for row in per_file.values()) for key in attribute.COUNTERS}
                projections[mode] = {"per_file": per_file, "aggregate": aggregate}
            reconciliations = {
                mode: attribute.reconcile(
                    projections["control-empty"]["aggregate"], projections[mode]["aggregate"]
                )
                for mode in (*attribute.TOKENS, "union")
            }
            receipt = {
                "schema": "genshare-receipt/v3",
                "status": "observed_unreviewed",
                "run_root": str(root.resolve()),
                "token_contract": {
                    "individual": list(attribute.TOKENS), "union": ",".join(attribute.TOKENS)
                },
                "selection": {
                    "ordered_paths": relative_paths,
                    "ordered_manifest": manifest_rows,
                    "manifest_sha256": attribute.canonical_sha256([
                        {key: row[key] for key in ("relative_path", "size", "mode", "sha256")}
                        for row in manifest_rows
                    ]),
                    "path_set_sha256": attribute.path_set_sha256(relative_paths),
                    "selected_files": 3,
                },
                "runs": runs,
                "projections": projections,
                "reconciliations": reconciliations,
                "pre_seam": None,
                "pre_seam_control": None,
                "failed_stage": None,
                "failure": None,
            }
            receipt["inertness"] = attribute._validate_inertness(runs, receipt["selection"])
            receipt["pre_seam_control"] = attribute._validate_pre_seam_control(
                runs, receipt["selection"]
            )
            receipt["fixture_contract"] = attribute._fixture_observations(
                runs, projections, reconciliations, {"sha256": "f" * 64}
            )
            proof_root = root / "pre-seam"
            proof_root.mkdir()
            retained_binary = proof_root / "oxidex"
            retained_binary.write_bytes(b"ordinary binary")
            built_binary = root / "outside-target" / "release" / "oxidex"
            built_binary.parent.mkdir(parents=True)
            built_binary.write_bytes(retained_binary.read_bytes())
            process_records = {}
            for label in ("clone", "checkout", "cargo"):
                stdout = proof_root / f"{label}.stdout"
                stderr = proof_root / f"{label}.stderr"
                stdout.write_bytes(b"")
                stderr.write_bytes(b"")
                process_records[label] = {
                    "returncode": 0,
                    "stdout": attribute._artifact_record(stdout),
                    "stderr": attribute._artifact_record(stderr),
                }
            parent = "1" * 40
            tree = "2" * 40
            proof = {
                "resolution": {
                    "path": "src/exiftool_tables/attribution.rs",
                    "introducing_commit": "3" * 40,
                    "introducing_tree": "4" * 40,
                    "introducing_parents": [parent],
                    "parent_commit": parent,
                    "parent_tree": tree,
                },
                "source_repository": str(root),
                "strategy": "run-owned shared clone with detached checkout; no protected ref mutation",
                "checkout": {
                    "root": str(root / "outside-checkout"),
                    "commit": parent,
                    "tree": tree,
                    "clean": True,
                    "dirty_files": [],
                },
                "target_dir": str(root / "outside-target"),
                "clone": process_records["clone"],
                "checkout_process": process_records["checkout"],
                "build": process_records["cargo"],
                "built_binary": attribute._artifact_record(built_binary),
                "binary": attribute._artifact_record(retained_binary),
                "run_mode": "pre-seam-control",
                "environment": attribute._mode_environment("pre-seam-control"),
            }
            proof_path = proof_root / "proof.json"
            attribute.write_json(proof_path, proof)
            proof["artifact"] = attribute._artifact_record(proof_path)
            receipt["pre_seam"] = proof
            receipt["artifact_index"] = attribute._artifact_index(root)
            attribute.validate_v3_receipt(receipt, root, replay=True)
            mutated = copy.deepcopy(receipt)
            mutated["pre_seam"]["resolution"]["parent_commit"] = "0" * 40
            with self.assertRaisesRegex(attribute.ReceiptError, "pre-seam"):
                attribute.validate_v3_receipt(mutated, root, replay=True)
            mutated = copy.deepcopy(receipt)
            mutated["projections"]["engine"]["aggregate"]["matched_occurrences"] += 1
            with self.assertRaisesRegex(attribute.ReceiptError, "equation|replay"):
                attribute.validate_v3_receipt(mutated, root, replay=True)
            tampered = copy.deepcopy(receipt)
            tampered["pre_seam"]["built_binary"]["sha256"] = "0" * 64
            attribute.write_json(
                proof_path,
                {key: value for key, value in tampered["pre_seam"].items() if key != "artifact"},
            )
            tampered["pre_seam"]["artifact"] = attribute._artifact_record(proof_path)
            proof_relative = proof_path.relative_to(root).as_posix()
            tampered["artifact_index"] = [
                attribute._file_identity(proof_path, relative_path=proof_relative)
                if row["relative_path"] == proof_relative else row
                for row in tampered["artifact_index"]
            ]
            with self.assertRaisesRegex(attribute.ReceiptError, "built.*retained|binary identity"):
                attribute.validate_v3_receipt(tampered, root, replay=True)


class RouteLedgerTests(unittest.TestCase):
    EXPECTED_BOUNDARY = (
        "src/exiftool_tables/attribution.rs",
        "src/exiftool_tables/engine.rs",
        "src/exiftool_tables/ifd_engine.rs",
        "src/exiftool_tables/keyed_engine.rs",
        "src/exiftool_tables/mod.rs",
        "src/exiftool_tables/runtime.rs",
        "src/exiftool_tables/serial_engine.rs",
        "src/main.rs",
        "src/composite/compute.rs",
        "src/composite/mod.rs",
        "src/core/file_metadata.rs",
        "src/core/operations.rs",
        "src/parsers/archive/ar.rs",
        "src/parsers/canon_vrd/mod.rs",
        "src/parsers/elf/metadata_extractor.rs",
        "src/parsers/flir_fpf.rs",
        "src/parsers/jpeg/app_segments/infiray.rs",
        "src/parsers/macho/metadata_extractor.rs",
        "src/parsers/specialized/fits.rs",
        "src/parsers/tiff/geotiff_parser.rs",
        "src/parsers/tiff/makernotes/canon/custom_functions2.rs",
        "src/parsers/tiff/makernotes/nikon/settings.rs",
        "src/parsers/tiff/makernotes/shared/binary_subdir.rs",
        "src/parsers/tiff/makernotes/sony.rs",
        "src/parsers/tiff/makernotes/sony/binary_data.rs",
    )

    def test_exact_task8_boundary_finds_repair_guards_and_keyed_stays_unreachable(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            repository = pathlib.Path(td) / "repo"
            run_root = pathlib.Path(td) / "run"
            (run_root / "contracts").mkdir(parents=True)
            for relative in self.EXPECTED_BOUNDARY:
                path = repository / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("// exact Task8 boundary fixture\n", encoding="utf-8")
            guards = {
                "src/exiftool_tables/engine.rs": "attribution::Token::Engine",
                "src/exiftool_tables/ifd_engine.rs": "attribution::Token::LegacyL1",
                "src/exiftool_tables/serial_engine.rs": "attribution::Token::Serial",
                "src/exiftool_tables/keyed_engine.rs": "attribution::Token::Keyed",
                "src/parsers/tiff/makernotes/sony.rs": "attribution::Token::LegacyL2",
                "src/composite/mod.rs": "attribution::Token::Producers",
            }
            for relative, source in guards.items():
                with (repository / relative).open("a", encoding="utf-8") as handle:
                    handle.write(source + "\n")
            with (repository / "src/main.rs").open("a", encoding="utf-8") as handle:
                handle.write("process_serial_directory(input);\n")

            ledger = attribute._route_ledger(repository, run_root)

            self.assertEqual(
                [row["path"] for row in ledger["sources"]],
                list(self.EXPECTED_BOUNDARY),
            )
            self.assertTrue(all(ledger["guard_sites"][token] for token in attribute.TOKENS))
            self.assertTrue(ledger["production_reachable"]["serial"])
            self.assertFalse(ledger["production_reachable"]["keyed"])

    def test_route_ledger_does_not_hide_production_after_cfg_test_helper(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            repository = pathlib.Path(td) / "repo"
            run_root = pathlib.Path(td) / "run"
            (run_root / "contracts").mkdir(parents=True)
            for relative in self.EXPECTED_BOUNDARY:
                path = repository / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("// exact Task8 boundary fixture\n", encoding="utf-8")
            guards = {
                "src/exiftool_tables/engine.rs": "attribution::Token::Engine",
                "src/exiftool_tables/ifd_engine.rs": "attribution::Token::LegacyL1",
                "src/exiftool_tables/serial_engine.rs": "attribution::Token::Serial",
                "src/exiftool_tables/keyed_engine.rs": "attribution::Token::Keyed",
                "src/parsers/tiff/makernotes/sony.rs": "attribution::Token::LegacyL2",
                "src/composite/mod.rs": "attribution::Token::Producers",
            }
            for relative, source in guards.items():
                with (repository / relative).open("a", encoding="utf-8") as handle:
                    handle.write(source + "\n")
            with (repository / "src/exiftool_tables/ifd_engine.rs").open(
                "a", encoding="utf-8"
            ) as handle:
                handle.write(
                    "#[cfg(test)]\nfn helper() {}\n"
                    "#[cfg(not(test))]\nfn helper() {}\n"
                    "fn production() { process_serial_directory(input); }\n"
                    "#[cfg(test)]\nmod tests {\n"
                    "    fn only_test() { process_keyed_directory(input); }\n"
                    "}\n"
                )

            ledger = attribute._route_ledger(repository, run_root)

            self.assertTrue(ledger["production_reachable"]["serial"])
            self.assertFalse(ledger["production_reachable"]["keyed"])


class PreSeamControlTests(unittest.TestCase):
    def test_introducing_commit_resolves_to_its_unique_parent(self):
        self.assertTrue(hasattr(attribute, "_resolve_pre_seam"))
        proof = attribute._resolve_pre_seam(ROOT)
        introducing = proof["introducing_commit"]
        parent = proof["parent_commit"]
        path = proof["path"]
        self.assertEqual(
            proof["introducing_parents"],
            attribute._git(ROOT, "show", "-s", "--format=%P", introducing).split(),
        )
        self.assertEqual(proof["introducing_parents"], [parent])
        self.assertEqual(
            proof["introducing_tree"],
            attribute._git(ROOT, "rev-parse", f"{introducing}^{{tree}}"),
        )
        self.assertEqual(
            proof["parent_tree"], attribute._git(ROOT, "rev-parse", f"{parent}^{{tree}}")
        )
        self.assertEqual(
            attribute._git(ROOT, "cat-file", "-t", f"{introducing}:{path}"), "blob"
        )
        absent = subprocess.run(
            ["git", "-C", str(ROOT), "cat-file", "-e", f"{parent}:{path}"],
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(absent.returncode, 0)

    def test_pre_seam_control_requires_normalized_output_and_stderr_equality(self):
        self.assertTrue(hasattr(attribute, "_validate_pre_seam_control"))
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            root = pathlib.Path(td)
            runs = {}
            for mode, value, stderr in (
                ("control-unset", "same", b""),
                ("pre-seam-control", "different", b""),
            ):
                child = root / mode
                child.mkdir()
                stderr_path = child / "candidate.stderr"
                stderr_path.write_bytes(stderr)
                process_path = child / "process.json"
                process_path.write_text(json.dumps({
                    "candidate_occurrences": [{"raw_key": "X:Y", "value": value}],
                    "candidate": {"stderr": {
                        "path": str(stderr_path),
                        "size": len(stderr),
                        "mode": stderr_path.stat().st_mode & 0o777,
                        "sha256": hashlib.sha256(stderr).hexdigest(),
                    }},
                }), encoding="utf-8")
                runs[mode] = {"children": [{
                    "relative_path": "ICC_Profile.icc",
                    "process": {"path": str(process_path)},
                }]}
            with self.assertRaisesRegex(attribute.ReceiptError, "pre-seam"):
                attribute._validate_pre_seam_control(
                    runs, {"ordered_paths": ["ICC_Profile.icc"]}
                )


if __name__ == "__main__":
    unittest.main()
