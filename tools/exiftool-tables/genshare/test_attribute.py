"""Contract tests for the authenticated generated-route attribution receipt."""

import copy
import hashlib
import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest


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
            oracle_json = '[{"File:FileType":"ICC","ICC_Profile:Header":"abc"}]'
            full_json = '[{"File:FileType":"ICC","ICC-header:Header":"abc"}]'
            drop_json = '[{"File:FileType":"ICC"}]'
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
            for mode in ("control-unset", "control-empty", *attribute.TOKENS, "union"):
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
                    candidate_text = (
                        drop_json if mode == "engine" and relative == "ICC_Profile.icc" else full_json
                    )
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
                    per_file[relative] = attribute.project_file(oracle_parsed, candidate_parsed)
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
                "failed_stage": None,
                "failure": None,
            }
            receipt["inertness"] = attribute._validate_inertness(runs, receipt["selection"])
            receipt["fixture_contract"] = attribute._fixture_observations(
                runs, projections, reconciliations, {"sha256": "f" * 64}
            )
            receipt["artifact_index"] = attribute._artifact_index(root)
            attribute.validate_v3_receipt(receipt, root, replay=True)
            mutated = copy.deepcopy(receipt)
            mutated["projections"]["engine"]["aggregate"]["matched_occurrences"] += 1
            with self.assertRaisesRegex(attribute.ReceiptError, "equation|replay"):
                attribute.validate_v3_receipt(mutated, root, replay=True)


if __name__ == "__main__":
    unittest.main()
