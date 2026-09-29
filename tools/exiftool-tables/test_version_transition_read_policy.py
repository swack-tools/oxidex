"""Pure, duplicate-aware Task19 read transition policy contracts."""

from __future__ import annotations

import hashlib
import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import version_transition_read_policy as policy
import conformance


def manifest(*rows: tuple[str, str, int]) -> bytes:
    return (json.dumps({
        "schema": 1,
        "kind": "oxidex_version_rehearsal_fixture_manifest",
        "fixtures": [{"path": path, "sha256": digest, "bytes": size}
                     for path, digest, size in rows],
    }, sort_keys=True) + "\n").encode()


class UnionTests(unittest.TestCase):
    def test_keeps_both_changed_bytes_and_original_manifest_provenance(self):
        before = manifest(
            ("/sources/11.78/t/images/AAC.aac", "a" * 64, 31),
            ("/sources/11.78/t/images/XMP.xml", "b" * 64, 100),
        )
        after = manifest(
            ("/sources/12.64/t/images/AAC.aac", "a" * 64, 31),
            ("/sources/12.64/t/images/XMP.xml", "c" * 64, 101),
            ("/sources/12.64/t/images/new/JXL.jxl", "d" * 64, 44),
        )
        result = policy.freeze_union(before, after)
        self.assertEqual(result["schema"], "task19-read-union/v1")
        self.assertEqual(result["manifest_sha256"], {
            "before": hashlib.sha256(before).hexdigest(),
            "after": hashlib.sha256(after).hexdigest(),
        })
        self.assertEqual([(r["logical_name"], r["sha256"]) for r in result["fixtures"]], [
            ("AAC.aac", "a" * 64), ("XMP.xml", "b" * 64),
            ("XMP.xml", "c" * 64), ("new/JXL.jxl", "d" * 64),
        ])
        self.assertEqual(result["fixtures"][0]["sources"], {
            "before": "/sources/11.78/t/images/AAC.aac",
            "after": "/sources/12.64/t/images/AAC.aac",
        })
        self.assertEqual(result["fixtures"][1]["sources"]["after"], None)
        self.assertEqual(result["fixtures"][2]["sources"]["before"], None)
        self.assertEqual(len(result["union_sha256"]), 64)

    def test_rejects_reused_logical_key_with_conflicting_sizes(self):
        before = manifest(("/a/t/images/AAC.aac", "a" * 64, 31))
        after = manifest(("/b/t/images/AAC.aac", "a" * 64, 32))
        with self.assertRaises(policy.ReadPolicyRefused):
            policy.freeze_union(before, after)

    def test_rejects_duplicate_logical_name_or_json_field(self):
        repeated = manifest(("/a/t/images/AAC.aac", "a" * 64, 31),
                            ("/b/t/images/AAC.aac", "b" * 64, 32))
        with self.assertRaises(policy.ReadPolicyRefused):
            policy.freeze_union(repeated, repeated)
        duplicate_field = (b'{"schema":1,"schema":1,"kind":'
                           b'"oxidex_version_rehearsal_fixture_manifest","fixtures":[]}')
        with self.assertRaises(policy.ReadPolicyRefused):
            policy.freeze_union(duplicate_field, duplicate_field)


class LedgerTests(unittest.TestCase):
    fixture = {"logical_name": "XMP.xml", "sha256": "f" * 64, "bytes": 100,
               "sources": {"before": "/a/t/images/XMP.xml", "after": "/b/t/images/XMP.xml"}}

    def transcript(self, oracle: dict, candidate: dict) -> dict:
        return conformance.transcript_row(
            "/staged/0000.xml", oracle, candidate, conformance.compare(oracle, candidate))

    def test_records_both_matched_full_oracle_keys_and_multiplicity(self):
        oracle = {"EXIF:IFD0:Copy1:CreateDate": "2020:01:01",
                  "EXIF:ExifIFD:Copy1:CreateDate": "2021:01:01"}
        candidate = {"IFD0:CreateDate": "2020:01:01",
                     "ExifIFD:CreateDate": "2021:01:01"}
        result = policy.occurrence_ledger(self.fixture, oracle, candidate,
                                          self.transcript(oracle, candidate), native_status=0)
        self.assertEqual(result["schema"], "task19-read-ledger/v1")
        self.assertEqual([(row["oracle_key"], row["value"]) for row in result["matched"]], [
            ("EXIF:ExifIFD:Copy1:CreateDate", "2021:01:01"),
            ("EXIF:IFD0:Copy1:CreateDate", "2020:01:01"),
        ])
        self.assertEqual(result["counts"]["matched"], 2)
        self.assertEqual(result["counts"]["missing"], 0)
        self.assertEqual(result["payload_occurrences"], 2)
        self.assertEqual(len(result["ledger_sha256"]), 64)

    def test_refuses_raw_map_that_differs_from_bound_transcript(self):
        oracle = {"EXIF:IFD0:Make": "Pentax"}
        candidate = {"IFD0:Make": "Pentax"}
        transcript = self.transcript(oracle, candidate)
        with self.assertRaises(policy.ReadPolicyRefused):
            policy.occurrence_ledger(self.fixture, oracle, {"IFD0:Make": "Canon"},
                                     transcript, native_status=0)

    def test_refuses_partial_ambiguous_duplicate_credit(self):
        oracle = {"EXIF:IFD0:Copy1:Make": "Pentax",
                  "EXIF:ExifIFD:Copy1:Make": "Pentax"}
        candidate = {"IFD0:Make": "Pentax"}
        with self.assertRaises(policy.ReadPolicyRefused):
            policy.occurrence_ledger(self.fixture, oracle, candidate,
                                     self.transcript(oracle, candidate), native_status=0)


class PairTests(unittest.TestCase):
    fixture = LedgerTests.fixture

    def ledger(self, native, candidate, *, status=0):
        transcript = conformance.transcript_row(
            "/staged/XMP.xml", native, candidate, conformance.compare(native, candidate))
        return policy.occurrence_ledger(self.fixture, native, candidate,
                                        transcript, native_status=status)

    def union(self):
        return policy.freeze_union(
            manifest(("/old/t/images/XMP.xml", "f" * 64, 100)),
            manifest(("/new/t/images/XMP.xml", "f" * 64, 100)))

    def replay(self, before, after, **changes):
        return policy.replay_pair(
            mode=changes.pop("mode", "same-pin"), union=self.union(),
            before_ledgers=changes.pop("before_ledgers", [before]),
            after_ledgers=changes.pop("after_ledgers", [after]),
            payload_floors=changes.pop("payload_floors", {"before": 1, "after": 1}),
            before_artifacts={"tables.json": "a" * 64},
            after_artifacts=changes.pop("after_artifacts", {"tables.json": "a" * 64}),
            native_change_evidence=changes.pop("native_change_evidence", []),
            artifact_change_evidence=changes.pop("artifact_change_evidence", []),
            **changes)

    def test_same_pin_allows_reported_standing_extra(self):
        native = {"EXIF:IFD0:Make": "Pentax"}
        candidate = {"IFD0:Make": "Pentax", "IFD0:Unknown": "extra"}
        before = self.ledger(native, candidate)
        result = self.replay(before, self.ledger(native, candidate))
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["standing"]["extra"], 1)
        self.assertEqual(result["new_losses"], [])

    def test_same_pin_rejects_changed_candidate_despite_equal_total(self):
        native = {"EXIF:IFD0:Make": "Pentax"}
        before = self.ledger(native, {"IFD0:Make": "Pentax"})
        after = self.ledger(native, {"IFD0:Make": "Canon"})
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after)

    def test_refuses_missing_carrier_and_nonpositive_payload_floor(self):
        native = {"EXIF:IFD0:Make": "Pentax"}
        row = self.ledger(native, {"IFD0:Make": "Pentax"})
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(row, row, after_ledgers=[])
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(row, row, payload_floors={"before": 0, "after": 1})

    def test_refuses_native_error_as_proven_read(self):
        native = {"ExifTool:Error": "Unknown file type"}
        row = self.ledger(native, {}, status=1)
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(row, row)

    def test_refuses_rehashed_but_fabricated_matched_ledger(self):
        native = {"EXIF:IFD0:Make": "Pentax"}
        row = self.ledger(native, {"IFD0:Make": "Pentax"})
        forged = json.loads(json.dumps(row))
        forged["matched"] = []
        forged["ledger_sha256"] = hashlib.sha256(policy._canonical_bytes(
            {key: value for key, value in forged.items()
             if key != "ledger_sha256"})).hexdigest()
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(forged, row)

    def test_historical_preserves_stable_matched_duplicate_count(self):
        native = {"EXIF:IFD0:Make": "Pentax", "EXIF:ExifIFD:Make": "Canon"}
        before = self.ledger(native, {"IFD0:Make": "Pentax", "ExifIFD:Make": "Canon"})
        after = self.ledger(native, {"IFD0:Make": "Pentax"})
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after, mode="historical")

    def test_historical_protects_stable_file_group_scored_read(self):
        native = {"EXIF:IFD0:Make": "Pentax", "File:ImageWidth": 640}
        before = self.ledger(native, {"IFD0:Make": "Pentax", "File:ImageWidth": 640})
        after = self.ledger(native, {"IFD0:Make": "Pentax"})
        with self.assertRaisesRegex(policy.ReadPolicyRefused,
                                    "previously matched native-supported read"):
            self.replay(before, after, mode="historical")

    def test_historical_classifies_new_native_only_missing_without_credit(self):
        old = {"EXIF:IFD0:Make": "Pentax"}
        new = {"EXIF:IFD0:Make": "Pentax", "EXIF:IFD0:Model": "Optio"}
        before = self.ledger(old, {"IFD0:Make": "Pentax"})
        after = self.ledger(new, {"IFD0:Make": "Pentax"})
        evidence = [{"logical_name": "XMP.xml", "fixture_sha256": "f" * 64,
                     "oracle_key": "EXIF:IFD0:Model", "before": None,
                     "after": "optio", "reason": "native-new-unread"}]
        result = self.replay(before, after, mode="historical",
                             native_change_evidence=evidence)
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["newly_supported_unread"], [
            {"logical_name": "XMP.xml", "fixture_sha256": "f" * 64,
             "oracle_key": "EXIF:IFD0:Model", "value": "optio",
             "missing_key": "Model"}])
        self.assertEqual(result["payload_occurrences"]["after"], 2)
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after, mode="historical",
                        native_change_evidence=[{**evidence[0], "reason": "native-version"}])

    def test_historical_requires_correct_changed_native_value_and_classification(self):
        old = {"EXIF:IFD0:Make": "Pentax"}
        new = {"EXIF:IFD0:Make": "Ricoh"}
        before = self.ledger(old, {"IFD0:Make": "Pentax"})
        after = self.ledger(new, {"IFD0:Make": "Ricoh"})
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after, mode="historical")
        evidence = [{"logical_name": "XMP.xml", "fixture_sha256": "f" * 64,
                     "oracle_key": "EXIF:IFD0:Make", "before": "pentax",
                     "after": "ricoh", "reason": "native-version"}]
        result = self.replay(before, after, mode="historical",
                             native_change_evidence=evidence)
        self.assertEqual(result["status"], "passed")
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after, mode="historical",
                        native_change_evidence=[{**evidence[0], "reason": "guess"}])
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, self.ledger(new, {"IFD0:Make": "Pentax"}),
                        mode="historical", native_change_evidence=evidence)

    def test_historical_rejects_removed_native_value_kept_as_extra(self):
        old = {"EXIF:IFD0:Make": "Pentax", "EXIF:IFD0:Model": "Optio"}
        new = {"EXIF:IFD0:Make": "Pentax"}
        before = self.ledger(old, {"IFD0:Make": "Pentax", "IFD0:Model": "Optio"})
        after = self.ledger(new, {"IFD0:Make": "Pentax", "IFD0:Model": "Optio"})
        evidence = [{"logical_name": "XMP.xml", "fixture_sha256": "f" * 64,
                     "oracle_key": "EXIF:IFD0:Model", "before": "optio",
                     "after": None, "reason": "native-version"}]
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after, mode="historical",
                        native_change_evidence=evidence)

    def test_historical_rejects_unclassified_new_extra_and_artifact_delta(self):
        native = {"EXIF:IFD0:Make": "Pentax"}
        before = self.ledger(native, {"IFD0:Make": "Pentax"})
        after = self.ledger(native, {"IFD0:Make": "Pentax", "IFD0:New": "x"})
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after, mode="historical")
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, before, mode="historical",
                        after_artifacts={"tables.json": "b" * 64})

    def test_historical_rejects_new_value_gap_even_if_prior_gap_stood(self):
        native = {"EXIF:IFD0:Make": "Pentax", "EXIF:IFD0:Model": "Optio"}
        before = self.ledger(native, {})
        after = self.ledger(native, {"IFD0:Make": "Canon"})
        with self.assertRaises(policy.ReadPolicyRefused):
            self.replay(before, after, mode="historical")

    def test_historical_reports_unchanged_standing_missing_without_credit(self):
        native = {"EXIF:IFD0:Make": "Pentax", "EXIF:IFD0:Model": "Optio"}
        row = self.ledger(native, {"IFD0:Make": "Pentax"})
        result = self.replay(row, row, mode="historical")
        self.assertEqual(result["standing"]["missing"], 1)
        self.assertEqual(result["new_losses"], [])

    def test_historical_accepts_exact_classified_artifact_change(self):
        native = {"EXIF:IFD0:Make": "Pentax"}
        row = self.ledger(native, {"IFD0:Make": "Pentax"})
        result = self.replay(
            row, row, mode="historical", after_artifacts={"tables.json": "b" * 64},
            artifact_change_evidence=[{"path": "tables.json", "before": "a" * 64,
                                       "after": "b" * 64,
                                       "reason": "generated-artifact"}])
        self.assertEqual(result["artifact_delta"][0]["path"], "tables.json")


if __name__ == "__main__":
    unittest.main()
