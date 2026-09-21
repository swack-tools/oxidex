#!/usr/bin/env python3
"""Contract tests for the checked Exif::Main refusal worklist."""

from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools" / "exiftool-tables"
LEDGER = TOOLS / "conv_exif_main_ledger.json"
WORKLIST = TOOLS / "exif_main_refusal_worklist.json"
HELPER_CAPTURE = TOOLS / "testdata" / "helper_oracle_outputs.json"


class RefusalWorklist(unittest.TestCase):
    def setUp(self):
        self.ledger = json.loads(LEDGER.read_text(encoding="utf-8"))
        self.worklist = json.loads(WORKLIST.read_text(encoding="utf-8"))

    def test_enumerates_the_exact_current_refusal_set(self):
        self.assertEqual(self.worklist["schema"], "oxidex_exif_main_refusal_worklist_v1")
        self.assertEqual(self.worklist["table"], "Exif::Main")
        self.assertEqual(self.worklist["source_release"], "13.59")
        rows = self.worklist["rows"]
        self.assertEqual(len(rows), 17)
        self.assertEqual(
            [(row["id"], row["name"], row["current_reason"]) for row in rows],
            [(row["id"], row["name"], row["reason"]) for row in self.ledger["refused"]],
        )

    def test_every_row_pins_source_probes_and_one_owner(self):
        allowed_owners = {"generated", "walker", "residual"}
        allowed_implementation_roots = ("src/", "tools/", "tests/")
        for row in self.worklist["rows"]:
            with self.subTest(id=row["id"], name=row["name"]):
                body = row["source_body"]
                self.assertIsInstance(body, str)
                self.assertTrue(body)
                self.assertEqual(
                    row["source_sha256"],
                    hashlib.sha256(body.encode("utf-8")).hexdigest(),
                )
                self.assertTrue(row["required_behavior"])
                self.assertTrue(row["probes"])
                self.assertTrue(all(isinstance(probe, str) and probe for probe in row["probes"]))
                self.assertIn(row["target_owner"], allowed_owners)
                self.assertTrue(row["implementation_file"].startswith(allowed_implementation_roots))
                self.assertTrue(row["named_test"])

                test_sources = [ROOT / row["implementation_file"]]
                test_sources.extend(
                    [
                        ROOT / "tests" / "exif_main_refusal_closure.rs",
                        ROOT / "tests" / "learning_opt_out_in.rs",
                    ]
                )
                definitions = "\n".join(
                    path.read_text(encoding="utf-8") for path in test_sources
                )
                self.assertIn(f"fn {row['named_test']}(", definitions)

    def test_count_or_classification_drift_requires_a_worklist_update(self):
        rows = {row["id"]: row for row in self.worklist["rows"]}
        ledger_ids = {row["id"] for row in self.ledger["refused"]}
        self.assertEqual(set(rows), ledger_ids)
        for ledger_row in self.ledger["refused"]:
            row = rows[ledger_row["id"]]
            self.assertEqual(row["ledger_status"], "refused")
            self.assertEqual(row["current_reason"], ledger_row["reason"])

    def test_behavioral_evidence_is_not_conflated_with_owner_classification(self):
        rows = {row["id"]: row for row in self.worklist["rows"]}
        for tag_id in ("0x00fe", "0x00ff", "0x0103"):
            self.assertEqual(rows[tag_id]["verification_status"], "planned_residual")
            self.assertEqual(
                rows[tag_id]["named_test"],
                "stateful_refusals_are_explicitly_planned_not_characterized",
            )
        for row in rows.values():
            if row["target_owner"] == "walker":
                self.assertEqual(row["verification_status"], "classified_owner")
                self.assertEqual(
                    row["named_test"],
                    "walker_owned_structural_refusals_are_classified_not_characterized",
                )

    def test_characterized_residuals_have_source_selected_native_capture(self):
        capture = json.loads(HELPER_CAPTURE.read_text(encoding="utf-8"))
        residuals = capture["residuals"]
        characterized = {
            row["id"]: row
            for row in self.worklist["rows"]
            if row["verification_status"] == "characterized_behavior"
        }
        self.assertEqual(set(residuals), set(characterized))
        for tag_id, row in characterized.items():
            with self.subTest(id=tag_id, name=row["name"]):
                proof = residuals[tag_id]
                self.assertEqual(proof["source_body"], row["source_body"])
                self.assertEqual(proof["source_sha256"], row["source_sha256"])
                self.assertTrue(proof["cases"])
                for case in proof["cases"]:
                    self.assertIn("stored", case)
                    self.assertIn("raw", case)
                    self.assertIn("value", case)
                    self.assertIn("print", case)

        time_codes = residuals["0xc763"]["cases"]
        inputs = {bytes.fromhex(case["input"]["hex"]): case for case in time_codes}
        self.assertEqual(
            bytes.fromhex(inputs[b"1 2 3 4 0 0 0 0"]["value"]["hex"]),
            b"01.02.03.04.00.00.00.00",
        )
        self.assertEqual(
            bytes.fromhex(inputs[b"0 0 0 128 1 1 122 0"]["print"]["hex"]),
            b"2007-01-01T00:00:00.00+00:00",
        )
        self.assertEqual(
            bytes.fromhex(inputs[b"0 0 0 128 1 0 30 128"]["print"]["hex"]),
            b"1858-11-27T00:00:00.00+00:00",
        )


if __name__ == "__main__":
    unittest.main()
