#!/usr/bin/env python3
"""Small fail-closed controls for Task18's unapproved historical deletions."""
from __future__ import annotations

import copy
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
import runtime_deletion_ledger as gate

ROOT = HERE.parents[1]


def field(value: str, owner: str, symbol: str, *, disposition: str | None = None) -> dict:
    return {"module": "Exif", "table": "Main", "field": {"kind": "numeric" if "/" not in value else "index", "value": value},
            "owner": owner, "symbol": symbol, "source_release": "13.59", "source_sha256": "a" * 64,
            "refusal": None, "residual_disposition": disposition}


class Task18DeletionGateControls(unittest.TestCase):
    def setUp(self) -> None:
        self.document = json.loads((ROOT / gate.LEDGER).read_text())

    def test_exact_five_historical_changes_and_49_keep_are_structurally_valid(self) -> None:
        self.assertEqual(gate.validate_document(self.document), self.document)
        self.assertEqual(sum(row["count"] for row in self.document["retained_groups"]), 49)
        self.assertTrue(all(row["qualification"].startswith("unqualified")
                            for row in self.document["historical_changes"]))

    def test_missing_or_forged_chronology_never_passes(self) -> None:
        mutations = []
        missing = copy.deepcopy(self.document)
        missing["historical_changes"] = []
        mutations.append(missing)
        changed_commit = copy.deepcopy(self.document)
        changed_commit["historical_changes"][0]["development_commit"] = "0" * 40
        mutations.append(changed_commit)
        approved = copy.deepcopy(self.document)
        approved["historical_changes"][0]["qualification"] = "approved"
        mutations.append(approved)
        widened_keep = copy.deepcopy(self.document)
        widened_keep["retained_groups"][0]["count"] = 3
        mutations.append(widened_keep)
        for document in mutations:
            with self.subTest(document=document), self.assertRaises(gate.Refused):
                gate.validate_document(document)

    def test_empty_appendix_and_prospective_entries_cannot_promote_history(self) -> None:
        with patch.object(gate, "verify_history", return_value="b" * 40), \
             patch.object(gate.ownership, "load_rows", return_value=[]):
            result = gate.verify(ROOT)
        self.assertEqual(result["status"], "BLOCKED")
        self.assertEqual(result["historical_unqualified"], 5)
        for key, value in (("approved_finite_appendix", {"attestation": "invented"}),
                           ("controller_reconciliation_manifest", {}),
                           ("prospective_entries", [{"old_symbol": "invented"}])):
            changed = copy.deepcopy(self.document)
            changed[key] = value
            with self.subTest(key=key), self.assertRaises(gate.Refused):
                gate.validate_document(changed)

    def test_existing_decline_fallback_is_retained_but_new_manual_owner_refuses(self) -> None:
        generator = field("0x9c9b", "generated", "src/exiftool_tables/conv/exif_main.rs::arm_9c9b")
        accepted = field("IFD0/0x9c9b", "residual", "src/core/exif_dir_engine.rs::IFD0_HAND_ON_DECLINE",
                         disposition="fallback-on-decline")
        gate.no_new_manual([generator, accepted])
        new = field("ExifIFD/0x9c9b", "residual", "src/core/tiff_helpers.rs::NEW_HAND_ARM",
                    disposition="hand-kept")
        with self.assertRaisesRegex(gate.Refused, "new manual owner"):
            gate.no_new_manual([generator, accepted, new])
        newly_generated = field("0x1234", "generated", "src/exiftool_tables/conv/exif_main.rs::arm_1234")
        old_residual = field("IFD0/0x1234", "residual", "src/core/exif_dir_engine.rs::IFD0_HAND_KEPT",
                             disposition="hand-kept")
        with self.assertRaisesRegex(gate.Refused, "new manual owner"):
            gate.no_new_manual([newly_generated, old_residual])

    def test_ownership_duplicate_is_not_a_deletion_waiver(self) -> None:
        first = field("0x1234", "generated", "src/exiftool_tables/conv/exif_main.rs::arm_1234")
        second = dict(first, symbol="src/core/tiff_helpers.rs::HAND_ARM")
        with self.assertRaisesRegex(gate.ownership.Refused, "duplicate owner"):
            gate.no_new_manual([first, second])


if __name__ == "__main__":
    unittest.main()
