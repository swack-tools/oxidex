#!/usr/bin/env python3
"""Small fail-closed controls for Task18's unapproved historical deletions."""
from __future__ import annotations

import copy
import hashlib
import shutil
import subprocess
import tempfile
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


class ProspectiveAuthenticatedPacketControls(unittest.TestCase):
    def test_signed_packet_acceptance_and_mutations(self) -> None:
        if not shutil.which("ssh-keygen"):
            self.skipTest("ssh-keygen unavailable")
        with tempfile.TemporaryDirectory(prefix="task18-packet-") as directory:
            ops = Path(directory)
            key = ops / "test-only-controller"
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)],
                           check=True, capture_output=True)
            trusted = key.with_suffix(".pub").read_bytes()
            binary_file = ops / "candidate-bin"
            binary_file.write_bytes(b"test-only-binary")
            binary = gate._sha(binary_file.read_bytes())
            source = "sha256:" + "b" * 64
            head = "c" * 40
            integration = gate.INTEGRATION_COMMIT
            symbol = "src/core/exif_dir_engine.rs::IFD0_HAND_KEPT"
            source_field = "Exif::Main:index:IFD0/0x9c9b"
            row = field("IFD0/0x9c9b", "generated", "src/exiftool_tables/conv/exif_main.rs::arm")
            candidate = {"old_symbol": symbol, "source_fields": [source_field], "new_owner": "generated",
                         "candidate_source_sha256": source, "candidate_binary_sha256": binary}

            def sign(name: str, document: dict) -> dict:
                path = ops / f"{name}.json"
                path.write_text(json.dumps(document, sort_keys=True))
                subprocess.run(["ssh-keygen", "-Y", "sign", "-f", str(key), "-n",
                                "oxidex-task18-controller", str(path)], check=True, capture_output=True)
                return {"path": str(path), "sha256": gate._sha(path.read_bytes()),
                        "signature_path": str(path) + ".sig"}

            appendix = sign("appendix", {"schema": "runtime-deletion-appendix/v1", "task": "18",
                        "approval": "prospective-approved", "candidate_commit": head,
                        "integration_sha": integration, "merge_sha": integration,
                        "source_sha256": source, "binary_sha256": binary, "candidates": [candidate]})
            common = {"schema": "runtime-deletion-evidence/v1", "task": "18", "old_symbol": symbol,
                      "source_fields": [source_field], "source_sha256": source, "binary_sha256": binary,
                      "integration_sha": integration, "merge_sha": integration}
            capability = sign("capability", dict(common, kind="oracle_capability", release="13.59",
                                                     perl="5.38.2", capability_probe="PASS"))
            fixture = "sha256:" + "f" * 64
            receipts = {"oracle_capability": capability,
                        "oracle": sign("oracle", dict(common, kind="oracle", capability_sha256=capability["sha256"],
                                                   matched_occurrences=1, lost_occurrences=0, new_value_rows=0,
                                                   fixture_sha256=fixture)),
                        "attribution": sign("attribution", dict(common, kind="attribution", generated_on="matched",
                                                                 generated_off="missing-or-residual", duplicate_owner=False)),
                        "zero_reachability": sign("reach", dict(common, kind="zero_reachability", reachable=False,
                                                                  remaining_callsites=[], definition_checked=True,
                                                                  fixture_sha256=fixture,
                                                                  capability_sha256=capability["sha256"]))}
            manifest = sign("manifest", {"schema": "runtime-deletion-manifest/v1", "task": "18",
                            "appendix_sha256": appendix["sha256"], "candidate_commit": head,
                            "source_sha256": source, "binary_sha256": binary,
                            "integration_sha": integration, "merge_sha": integration,
                            "receipts": {symbol: {kind: binding["sha256"] for kind, binding in receipts.items()}}})
            entry = dict(candidate, receipt_task="18", receipt_integration_sha=integration,
                         receipt_merge_sha=integration,
                         controller_reconciliation_manifest_sha256=manifest["sha256"],
                         generated_on="matched", generated_off="missing-or-residual",
                         receipt_bindings=receipts, deletion_commit=None)
            packet = {"schema": "runtime-deletion-packet/v1", "appendix": appendix,
                      "manifest": manifest, "entries": [entry]}
            original = subprocess.run
            def bounded_run(args, *a, **kw):
                if isinstance(args, list) and "merge-base" in args:
                    return subprocess.CompletedProcess(args, 0)
                return original(args, *a, **kw)
            with patch.object(gate, "verify_history", return_value=head), \
                 patch.object(gate.clean_snapshot, "source_tree_sha256", return_value="b" * 64), \
                 patch.object(gate.ownership, "load_rows", return_value=[row]), \
                 patch.object(gate.subprocess, "run", side_effect=bounded_run):
                # Only ancestry and heavy source inventory are test seams.
                # The real ssh-keygen verifies every signed byte.
                self.assertEqual(gate.evaluate_prospective(packet, root=ROOT, ops_root=ops,
                                 binary_path=binary_file, controller_key=trusted,
                                 test_only_attribution_bridge=lambda _attr, fields: fields == [source_field])["status"],
                                 "PASS_TEST_PACKET_BINDINGS")
                changed = copy.deepcopy(packet)
                changed["entries"][0]["receipt_bindings"]["oracle"]["sha256"] = "sha256:" + "0" * 64
                with self.assertRaisesRegex(gate.Refused, "digest"):
                    gate.evaluate_prospective(changed, root=ROOT, ops_root=ops,
                                             binary_path=binary_file, controller_key=trusted,
                                             test_only_attribution_bridge=lambda _attr, _fields: True)
                changed = copy.deepcopy(packet)
                changed["entries"][0]["source_fields"] = ["Exif::Main:index:IFD0/0x9c9c"]
                with self.assertRaises(gate.Refused):
                    gate.evaluate_prospective(changed, root=ROOT, ops_root=ops,
                                             binary_path=binary_file, controller_key=trusted,
                                             test_only_attribution_bridge=lambda _attr, _fields: True)
                with self.assertRaisesRegex(gate.Refused, "BLOCKED_ATTRIBUTION"):
                    gate.evaluate_prospective(packet, root=ROOT, ops_root=ops,
                                             binary_path=binary_file, controller_key=trusted)
                with self.assertRaisesRegex(gate.Refused, "BLOCKED_AUTHORITY"):
                    gate.evaluate_prospective(packet, root=ROOT, ops_root=ops,
                                             binary_path=binary_file, controller_key=None)


if __name__ == "__main__":
    unittest.main()
