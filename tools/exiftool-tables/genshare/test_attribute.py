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
    def test_reviewed_fixture_hashes_allow_an_additional_selected_crw(self):
        document_path = MODULE.parent / "testdata/bounded-corpus-expectations.json"
        document = json.loads(document_path.read_text())
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            root = pathlib.Path(td)
            manifest = root / "selected.txt"
            manifest.write_text("ICC_Profile.icc\nAAC.aac\nOOXML.docx\nCanonRaw.crw\n")
            (root / "bounded-corpus-expectations.json").write_bytes(document_path.read_bytes())
            selection = {"ordered_manifest": [
                {"relative_path": row["relative_path"], "sha256": row["sha256"]}
                for row in document["fixtures"]
            ] + [{"relative_path": "CanonRaw.crw", "sha256": "a" * 64}]}
            loaded = attribute._load_expectations(manifest, selection, root)
            self.assertEqual(loaded["document"], document)
            selection["ordered_manifest"][1]["sha256"] = "0" * 64
            with self.assertRaisesRegex(attribute.ReceiptError, "fixture paths or content hashes"):
                attribute._load_expectations(manifest, selection, root)

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

    def test_combined_corpus_keeps_fixture_controls_and_records_crw_keyed_loss(self):
        fixtures = ["ICC_Profile.icc", "AAC.aac", "OOXML.docx"]
        paths = [*fixtures, "CanonRaw.crw"]
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            root = pathlib.Path(td)
            runs = {}
            projections = {}
            for mode in ("control-empty", *attribute.TOKENS, "union"):
                children = []
                per_file = {}
                for relative in paths:
                    oracle = {
                        "File:FileType": "CRW" if relative == "CanonRaw.crw" else "TEST",
                        "File:FileTypeExtension": "crw",
                        "File:MIMEType": "image/x-canon-crw",
                        "Test:Payload": relative,
                    }
                    candidate = dict(oracle)
                    if relative == "ICC_Profile.icc" and mode in ("engine", "union"):
                        candidate.pop("Test:Payload")
                    if mode in ("producers", "union"):
                        for key in ("File:FileType", "File:FileTypeExtension", "File:MIMEType"):
                            candidate.pop(key)
                    if relative == "CanonRaw.crw" and mode in ("keyed", "union"):
                        candidate.pop("Test:Payload")
                    child = root / mode / relative
                    child.parent.mkdir(parents=True, exist_ok=True)
                    child.write_text(json.dumps({"candidate_occurrences": attribute.occurrence_sequence(candidate, normalize_access_date=True)}))
                    children.append({"relative_path": relative, "process": {"path": str(child)}})
                    per_file[relative] = attribute.project_file(oracle, candidate)
                runs[mode] = {"children": children}
                projections[mode] = {"per_file": per_file, "aggregate": attribute._sum_projection(per_file)}
            reconciliations = {
                mode: attribute.reconcile(projections["control-empty"]["aggregate"], projections[mode]["aggregate"])
                for mode in (*attribute.TOKENS, "union")
            }
            observed = attribute._fixture_observations(
                runs, projections, reconciliations, {"sha256": "f" * 64}
            )
            payload = observed["observed_loss_payload"]
            self.assertEqual(payload["corpus"], paths)
            self.assertEqual(payload["modes"]["keyed"]["matched_lost"], 1)
            self.assertEqual(payload["modes"]["keyed"]["missing_by_file"]["CanonRaw.crw"], 1)
            for mode, reconciliation in reconciliations.items():
                self.assertEqual(payload["modes"][mode]["matched_lost"], reconciliation["matched_lost"])
            self.assertEqual(observed["keyed_sequence_deltas"]["OOXML.docx"]["removed"], [])
            fixture_projections = {
                mode: {
                    "per_file": {relative: projection["per_file"][relative] for relative in fixtures},
                    "aggregate": attribute._sum_projection({
                        relative: projection["per_file"][relative] for relative in fixtures
                    }),
                }
                for mode, projection in projections.items()
            }
            fixture_reconciliations = {
                mode: attribute.reconcile(fixture_projections["control-empty"]["aggregate"], fixture_projections[mode]["aggregate"])
                for mode in (*attribute.TOKENS, "union")
            }
            reviewed_payload = attribute._fixture_observations(
                runs, fixture_projections, fixture_reconciliations, {"sha256": "f" * 64}
            )["observed_loss_payload"]
            unchanged_three = attribute._fixture_observations(
                runs, fixture_projections, fixture_reconciliations,
                {"sha256": "f" * 64, "document": {
                    "review_status": "reviewed_exact", "exact_loss_expectations": reviewed_payload,
                }},
            )
            self.assertEqual(unchanged_three["reviewed_scope"], "selected_corpus")
            reviewed = attribute._fixture_observations(
                runs, projections, reconciliations,
                {"sha256": "f" * 64, "document": {
                    "review_status": "reviewed_exact", "exact_loss_expectations": reviewed_payload,
                }},
            )
            self.assertEqual(reviewed["reviewed_scope"], "bounded_controls")
            self.assertNotEqual(reviewed["observed_loss_payload"], reviewed["exact_loss_expectations"])
            wrong_review = copy.deepcopy(reviewed_payload)
            wrong_review["modes"]["keyed"]["matched_lost"] = 1
            with self.assertRaisesRegex(attribute.ReceiptError, "reviewed exact"):
                attribute._fixture_observations(
                    runs, projections, reconciliations,
                    {"sha256": "f" * 64, "document": {
                        "review_status": "reviewed_exact", "exact_loss_expectations": wrong_review,
                    }},
                )
            baseline = list(runs["keyed"]["children"])
            for relative in fixtures:
                changed = root / "keyed" / relative
                original = changed.read_text()
                rows = json.loads(original)["candidate_occurrences"]
                changed.write_text(json.dumps({"candidate_occurrences": rows[:-1]}))
                with self.subTest(relative=relative), self.assertRaisesRegex(attribute.ReceiptError, "keyed changed|AAC hand-only"):
                    attribute._fixture_observations(runs, projections, reconciliations, {"sha256": "f" * 64})
                changed.write_text(original)
            self.assertEqual(runs["keyed"]["children"], baseline)

    def test_validator_replays_four_file_reviewed_exact_and_rejects_tampering(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            root = pathlib.Path(td)
            relative_paths = ["ICC_Profile.icc", "AAC.aac", "OOXML.docx", "CanonRaw.crw"]
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
                    if mode == "pre-seam-control" and relative == "CanonRaw.crw":
                        candidate_document["Test:HistoricalParserValue"] = "before improvement"
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
                    "selected_files": len(relative_paths),
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
            expectations_path = root / "contracts" / "bounded-corpus-expectations.json"
            expectations_path.parent.mkdir()
            expectations_document = {
                "review_status": "roles_only",
                "fixtures": [{"relative_path": row["relative_path"], "sha256": row["sha256"]} for row in manifest_rows],
            }
            attribute.write_json(expectations_path, expectations_document)
            receipt["fixture_contract"]["expectations_sha256"] = attribute.sha256_file(expectations_path)
            observed_payload = receipt["fixture_contract"]["observed_loss_payload"]
            reviewed = attribute._fixture_observations(
                runs,
                projections,
                reconciliations,
                {
                    "sha256": "e" * 64,
                    "document": {
                        "review_status": "reviewed_exact",
                        "exact_loss_expectations": observed_payload,
                    },
                },
            )
            self.assertEqual(reviewed["review_status"], "reviewed_exact")
            self.assertEqual(reviewed["reviewed_scope"], "selected_corpus")
            self.assertEqual(reviewed["exact_loss_expectations"], observed_payload)
            self.assertEqual(observed_payload["corpus"], relative_paths)
            self.assertEqual(
                list(observed_payload["modes"]["engine"]["missing_by_file"]),
                relative_paths,
            )
            complete_document = {
                "review_status": "reviewed_exact",
                "fixtures": [
                    {"relative_path": row["relative_path"], "sha256": row["sha256"]}
                    for row in manifest_rows
                ],
                "exact_loss_expectations": observed_payload,
            }
            attribute._validate_expectations_document(complete_document, receipt["selection"])
            bounded_document = copy.deepcopy(complete_document)
            bounded_document["fixtures"] = bounded_document["fixtures"][:3]
            bounded_document["exact_loss_expectations"] = copy.deepcopy(observed_payload)
            bounded_document["exact_loss_expectations"]["corpus"] = relative_paths[:3]
            attribute._validate_fixture_selection(bounded_document, receipt["selection"])
            for label, change in (
                ("missing", lambda rows: rows.pop()),
                ("reordered", lambda rows: rows.reverse()),
                ("hash mismatch", lambda rows: rows[-1].update(sha256="0" * 64)),
                ("duplicate", lambda rows: rows.__setitem__(-1, copy.deepcopy(rows[0]))),
            ):
                malformed = copy.deepcopy(complete_document)
                change(malformed["fixtures"])
                with self.subTest(label=label), self.assertRaises(attribute.ReceiptError):
                    attribute._validate_expectations_document(malformed, receipt["selection"])
            wrong_payloads = []
            wrong = copy.deepcopy(observed_payload)
            wrong["modes"]["engine"]["matched_lost"] += 1
            wrong_payloads.append(wrong)
            wrong = copy.deepcopy(observed_payload)
            wrong["modes"]["engine"]["missing_by_file"]["CanonRaw.crw"] += 1
            wrong_payloads.append(wrong)
            wrong = copy.deepcopy(observed_payload)
            del wrong["modes"]["engine"]["missing_by_file"]["CanonRaw.crw"]
            wrong_payloads.append(wrong)
            wrong = copy.deepcopy(observed_payload)
            wrong["corpus"] = list(reversed(wrong["corpus"]))
            wrong_payloads.append(wrong)
            wrong = copy.deepcopy(observed_payload)
            wrong["schema"] = "genshare-exact-loss/v0"
            wrong_payloads.append(wrong)
            for wrong_payload in wrong_payloads:
                with self.subTest(wrong_payload=wrong_payload), self.assertRaisesRegex(
                    attribute.ReceiptError, "reviewed exact"
                ):
                    attribute._fixture_observations(
                        runs,
                        projections,
                        reconciliations,
                        {
                            "sha256": "d" * 64,
                            "document": {
                                "review_status": "reviewed_exact",
                                "exact_loss_expectations": wrong_payload,
                            },
                        },
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
            observed_key_orders = []
            original_project_file = attribute.project_file

            def record_project_order(oracle, candidate):
                observed_key_orders.append((list(oracle), list(candidate)))
                return original_project_file(oracle, candidate)

            with mock.patch.object(
                attribute, "project_file", side_effect=record_project_order
            ):
                attribute.validate_v3_receipt(receipt, root, replay=True)
            forged_success = copy.deepcopy(receipt)
            forged_success["status"] = "success"
            with self.assertRaisesRegex(attribute.ReceiptError, "reviewed corpus scope"):
                attribute.validate_v3_receipt(forged_success, root, replay=True)
            attribute.write_json(expectations_path, complete_document)
            receipt["fixture_contract"] = attribute._fixture_observations(
                runs, projections, reconciliations,
                {"sha256": attribute.sha256_file(expectations_path), "document": complete_document},
            )
            receipt["status"] = "success"
            receipt["artifact_index"] = attribute._artifact_index(root)
            attribute.validate_v3_receipt(receipt, root, replay=True)
            self.assertEqual(receipt["fixture_contract"]["reviewed_scope"], "selected_corpus")
            self.assertEqual(
                receipt["pre_seam_control"]["historical_full_selection"]["differences"],
                ["CanonRaw.crw"],
            )
            self.assertFalse(receipt["pre_seam_control"]["historical_full_selection"]["equal"])
            old = copy.deepcopy(receipt)
            old_paths = relative_paths[:3]
            old["selection"]["ordered_paths"] = old_paths
            old["selection"]["ordered_manifest"] = manifest_rows[:3]
            old["selection"]["selected_files"] = 3
            old["selection"]["path_set_sha256"] = attribute.path_set_sha256(old_paths)
            old["selection"]["manifest_sha256"] = attribute.canonical_sha256([
                {key: row[key] for key in ("relative_path", "size", "mode", "sha256")}
                for row in manifest_rows[:3]
            ])
            for mode in old["runs"]:
                old["runs"][mode]["children"] = old["runs"][mode]["children"][:3]
                old["runs"][mode]["path_set_sha256"] = attribute.path_set_sha256(old_paths)
                old["projections"][mode]["per_file"].pop("CanonRaw.crw")
                old["projections"][mode]["aggregate"] = attribute._sum_projection(
                    old["projections"][mode]["per_file"]
                )
            old["reconciliations"] = {
                mode: attribute.reconcile(
                    old["projections"]["control-empty"]["aggregate"],
                    old["projections"][mode]["aggregate"],
                ) for mode in (*attribute.TOKENS, "union")
            }
            old_document = copy.deepcopy(complete_document)
            old_document["fixtures"] = old_document["fixtures"][:3]
            old_document["exact_loss_expectations"] = attribute._fixture_observations(
                old["runs"], old["projections"], old["reconciliations"],
                {"sha256": "f" * 64},
            )["observed_loss_payload"]
            attribute.write_json(expectations_path, old_document)
            old["inertness"] = attribute._validate_inertness(old["runs"], old["selection"])
            old["pre_seam_control"] = attribute._validate_pre_seam_control(
                old["runs"], old["selection"]
            )
            old["pre_seam_control"].pop("historical_full_selection")
            old["pre_seam_control"].pop("compared_paths")
            old["fixture_contract"] = attribute._fixture_observations(
                old["runs"], old["projections"], old["reconciliations"],
                {"sha256": attribute.sha256_file(expectations_path), "document": old_document},
            )
            old["fixture_contract"].pop("reviewed_scope")
            old["artifact_index"] = attribute._artifact_index(root)
            attribute.validate_v3_receipt(old, root, replay=True)
            expanded_without_scope = copy.deepcopy(receipt)
            expanded_without_scope["fixture_contract"].pop("reviewed_scope")
            attribute.write_json(expectations_path, complete_document)
            expanded_without_scope["artifact_index"] = attribute._artifact_index(root)
            with self.assertRaisesRegex(attribute.ReceiptError, "fixture observations"):
                attribute.validate_v3_receipt(expanded_without_scope, root, replay=True)
            receipt["artifact_index"] = attribute._artifact_index(root)
            for label, change in (
                ("missing selected", lambda rows: rows.pop()),
                ("reordered selected", lambda rows: rows.reverse()),
                ("hash-mismatched selected", lambda rows: rows[-1].update(sha256="0" * 64)),
            ):
                forged = copy.deepcopy(receipt)
                change(forged["selection"]["ordered_manifest"])
                with self.subTest(label=label), self.assertRaises(attribute.ReceiptError):
                    attribute.validate_v3_receipt(forged, root, replay=True)
            forged = copy.deepcopy(receipt)
            forged["fixture_contract"]["observed_loss_payload"]["modes"]["engine"]["matched_lost"] += 1
            with self.assertRaisesRegex(attribute.ReceiptError, "fixture observations do not replay"):
                attribute.validate_v3_receipt(forged, root, replay=True)
            self.assertTrue(observed_key_orders)
            self.assertTrue(
                all(
                    oracle_keys == sorted(oracle_keys)
                    and candidate_keys == sorted(candidate_keys)
                    for oracle_keys, candidate_keys in observed_key_orders
                )
            )
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
        "src/parsers/raw/metadata.rs",
        "src/parsers/specialized/fits.rs",
        "src/parsers/tiff/geotiff_parser.rs",
        "src/parsers/tiff/makernotes/canon/custom_functions2.rs",
        "src/parsers/tiff/makernotes/nikon/settings.rs",
        "src/parsers/tiff/makernotes/shared/binary_subdir.rs",
        "src/parsers/tiff/makernotes/sony.rs",
        "src/parsers/tiff/makernotes/sony/binary_data.rs",
    )

    def test_exact_task8_boundary_finds_repair_guards_and_keyed_carrier(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            repository = pathlib.Path(td) / "repo"
            run_root = pathlib.Path(td) / "run"
            (run_root / "contracts").mkdir(parents=True)
            for relative in self.EXPECTED_BOUNDARY:
                path = repository / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(
                    (ROOT / relative).read_text(encoding="utf-8")
                    if relative == "src/parsers/raw/metadata.rs"
                    else "// exact Task8 boundary fixture\n",
                    encoding="utf-8",
                )
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
            self.assertTrue(ledger["production_reachable"]["keyed"])
            route = repository / "src/parsers/raw/metadata.rs"
            original = route.read_text(encoding="utf-8")
            call = "let result = process_keyed_directory(table, block, &mut ctx, &mut sink);"
            self.assertEqual(original.count(call), 1)
            for replacement in (
                "// " + call,
                "if false { " + call + " }",
                "",
            ):
                with self.subTest(replacement=replacement):
                    changed = original.replace(call, replacement)
                    if not replacement:
                        changed += "\n// process_keyed_directory(table, block, &mut ctx, &mut sink);\n"
                    route.write_text(changed, encoding="utf-8")
                    with self.assertRaisesRegex(
                        attribute.ReceiptError, "bounded firmware projection or caller"
                    ):
                        attribute._route_ledger(repository, run_root)
            # A body-only digest accepts these context changes: the bounded
            # function bytes remain, but the route is no longer compiled.
            name = "fn canon_firmware_from_keyed_entry("
            start = original.rfind("\n", 0, original.index(name)) + 1
            end = original.index("\n}\n", start) + 2
            function = original[start:end]
            context_mutations = {
                "whole_function_commented": (
                    original[:start] + "/*\n" + function + "\n*/" + original[end:]
                ),
                "cfg_disabled": original[:start] + "#[cfg(any())]\n" + original[start:],
                "duplicate_fake_definition": original + "\n#[cfg(any())]\n" + function + "\n",
            }
            for label, changed in context_mutations.items():
                with self.subTest(context=label):
                    route.write_text(changed, encoding="utf-8")
                    with self.assertRaisesRegex(
                        attribute.ReceiptError, "bounded firmware projection or caller"
                    ):
                        attribute._route_ledger(repository, run_root)
            route.write_text(
                original.replace("canon_firmware_from_keyed_entry(entry)", "None"),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                attribute.ReceiptError, "bounded firmware projection or caller"
            ):
                attribute._route_ledger(repository, run_root)
            route.write_text(original, encoding="utf-8")

    def test_route_ledger_does_not_hide_production_after_cfg_test_helper(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            repository = pathlib.Path(td) / "repo"
            run_root = pathlib.Path(td) / "run"
            (run_root / "contracts").mkdir(parents=True)
            for relative in self.EXPECTED_BOUNDARY:
                path = repository / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(
                    (ROOT / relative).read_text(encoding="utf-8")
                    if relative == "src/parsers/raw/metadata.rs"
                    else "// exact Task8 boundary fixture\n",
                    encoding="utf-8",
                )
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
            self.assertTrue(ledger["production_reachable"]["keyed"])
            self.assertEqual(len(ledger["production_calls"]["keyed"]), 1)


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

    def test_historical_control_scope_and_full_current_inertness(self):
        paths = ["ICC_Profile.icc", "AAC.aac", "OOXML.docx", "CanonRaw.crw"]
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            root = pathlib.Path(td)
            runs = {}
            records = {}
            for mode in ("pre-seam-control", "control-unset", "control-empty"):
                children = []
                for path in paths:
                    process_path = root / mode / f"{path}.json"
                    process_path.parent.mkdir(parents=True, exist_ok=True)
                    value = "old parser" if mode == "pre-seam-control" and path == "CanonRaw.crw" else "current"
                    process_path.write_text(json.dumps({
                        "candidate_occurrences": [{"raw_key": "X:Y", "value": value}],
                        "candidate": {"stderr": {"sha256": "0" * 64}},
                    }))
                    records[(mode, path)] = process_path
                    children.append({"relative_path": path, "process": {"path": str(process_path)}})
                runs[mode] = {"children": children}
            selection = {"ordered_paths": paths}
            historical = attribute._validate_pre_seam_control(runs, selection)
            self.assertEqual(historical["compared_paths"], paths[:3])
            self.assertEqual(historical["historical_full_selection"]["compared_paths"], paths)
            self.assertEqual(
                historical["historical_full_selection"]["path_set_sha256"],
                attribute.path_set_sha256(paths),
            )
            self.assertEqual(historical["historical_full_selection"]["differences"], ["CanonRaw.crw"])
            self.assertTrue(attribute._validate_inertness(runs, selection)["equal"])
            for path in paths[:3]:
                record = records[("pre-seam-control", path)]
                original = record.read_text()
                changed = json.loads(original)
                changed["candidate_occurrences"][0]["value"] = "different"
                record.write_text(json.dumps(changed))
                with self.subTest(control=path), self.assertRaisesRegex(
                    attribute.ReceiptError, "historical controls"
                ):
                    attribute._validate_pre_seam_control(runs, selection)
                record.write_text(original)
            current = records[("control-empty", "CanonRaw.crw")]
            changed = json.loads(current.read_text())
            changed["candidate_occurrences"][0]["value"] = "current drift"
            current.write_text(json.dumps(changed))
            with self.assertRaisesRegex(attribute.ReceiptError, "unset and empty controls differ"):
                attribute._validate_inertness(runs, selection)


if __name__ == "__main__":
    unittest.main()
