#!/usr/bin/env python3
"""Small fail-closed controls for Task18's unapproved historical deletions."""
from __future__ import annotations

import contextlib
import copy
import hashlib
import io
import os
import shutil
import subprocess
import tempfile
import json
import struct
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))
import runtime_deletion_ledger as gate
import runtime_field_projection as field_projection

ROOT = HERE.parents[1]


def field(value: str, owner: str, symbol: str, *, disposition: str | None = None) -> dict:
    return {"module": "Exif", "table": "Main", "field": {"kind": "numeric" if "/" not in value else "index", "value": value},
            "owner": owner, "symbol": symbol, "source_release": "13.59", "source_sha256": "a" * 64,
            "refusal": None, "residual_disposition": disposition}


class Task18DeletionGateControls(unittest.TestCase):
    def setUp(self) -> None:
        self.document = json.loads((ROOT / gate.LEDGER).read_text())

    def test_ci_runs_standalone_no_new_manual_after_pinned_baseline(self) -> None:
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        job = workflow.split('\n  verify-tables-tools:\n', 1)[1].split('\n  verify-tables-drift:\n', 1)[0]
        fetch = job.index('Fetch pinned Task18 generated-owner baseline')
        control = job.index('python3 tools/exiftool-tables/runtime_deletion_ledger.py no-new-manual --root .')
        self.assertLess(fetch, control)
        self.assertIn('git fetch --no-tags --depth=1 origin "$TASK18_BASELINE_COMMIT"', job[:control])
        self.assertNotIn('runtime_deletion_ledger.py verify --root .', job)

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

    def test_candidate_cannot_erase_pinned_generated_owner_before_manual_transfer(self) -> None:
        baseline = gate.generated_baseline(ROOT)
        self.assertIn(("Exif", "Main", "0x9c9b"), baseline)
        # The candidate contains no generated 0x9c9b row at all. A relabeled
        # hand-kept residual must still be compared with the pinned parent.
        transferred = field("IFD0/0x9c9b", "residual",
                            "src/core/exif_dir_engine.rs::IFD0_HAND_KEPT",
                            disposition="hand-kept")
        with self.assertRaisesRegex(gate.Refused, "new manual owner"):
            gate.no_new_manual([transferred], root=ROOT)

    def test_no_new_manual_cli_reaches_pinned_baseline_and_refuses_transfer(self) -> None:
        argv = ["runtime_deletion_ledger.py", "no-new-manual", "--root", str(ROOT)]
        fallback = field("IFD0/0x9c9b", "residual",
                         "src/core/exif_dir_engine.rs::IFD0_HAND_ON_DECLINE",
                         disposition="fallback-on-decline")
        transferred = field("IFD0/0x9c9b", "residual",
                            "src/core/exif_dir_engine.rs::IFD0_HAND_KEPT",
                            disposition="hand-kept")
        for rows, expected_status in (([fallback], 0), ([transferred], 2)):
            with self.subTest(status=expected_status), \
                 patch.object(gate.ownership, "load_rows", return_value=rows) as loaded, \
                 patch.object(sys, "argv", argv), \
                 contextlib.redirect_stdout(io.StringIO()) as output, \
                 contextlib.redirect_stderr(io.StringIO()) as errors:
                status = gate.main()
            self.assertEqual(status, expected_status)
            loaded.assert_called_once_with(ROOT.resolve(), None)
            if expected_status == 0:
                self.assertEqual(json.loads(output.getvalue())["status"], "PASS")
                self.assertEqual(errors.getvalue(), "")
            else:
                self.assertIn("new manual owner", errors.getvalue())
                self.assertEqual(output.getvalue(), "")

    def test_ownership_duplicate_is_not_a_deletion_waiver(self) -> None:
        first = field("0x1234", "generated", "src/exiftool_tables/conv/exif_main.rs::arm_1234")
        second = dict(first, symbol="src/core/tiff_helpers.rs::HAND_ARM")
        with self.assertRaisesRegex(gate.ownership.Refused, "duplicate owner"):
            gate.no_new_manual([first, second])


class DurableEvidencePathControls(unittest.TestCase):
    def test_temporary_and_root_evidence_roots_refuse(self) -> None:
        with tempfile.TemporaryDirectory(prefix="task18-path-") as directory:
            file = Path(directory) / "receipt.json"
            file.write_text("{}")
            for root in (Path("/"), Path("/tmp"), Path("/private/tmp"), Path(directory)):
                with self.subTest(root=root), self.assertRaisesRegex(gate.Refused, "durable"):
                    gate._file(str(file), root)

    def test_symlink_ancestor_refuses_even_inside_evidence_root(self) -> None:
        with tempfile.TemporaryDirectory(prefix="task18-path-") as directory:
            root = Path(directory)
            real = root / "real"
            real.mkdir()
            file = real / "receipt.json"
            file.write_text("{}")
            (root / "alias").symlink_to(real, target_is_directory=True)
            with self.assertRaisesRegex(gate.Refused, "symlink"):
                gate._file(str(root / "alias" / file.name), root, test_only_temporary_evidence=True)


class CandidateHistoryIdentityControls(unittest.TestCase):
    def test_fake_path_git_and_ambient_git_overrides_cannot_forge_history(self) -> None:
        system_git = Path("/usr/bin/git")
        if not system_git.is_file():
            self.skipTest("fixed system Git unavailable")
        with tempfile.TemporaryDirectory(prefix="task18-git-authority-") as directory:
            base = Path(directory)
            fake_dir = base / "fake-bin"
            fake_dir.mkdir()
            fake = fake_dir / "git"
            fake.write_text("#!/bin/sh\ncase \"$*\" in *status*) exit 0;; *write-tree*|*rev-parse*) echo 1111111111111111111111111111111111111111;; esac\nexit 0\n")
            fake.chmod(0o755)
            nonrepo = base / "dirty-nonrepo"
            nonrepo.mkdir()
            (nonrepo / "untracked").write_text("dirty")
            hostile = {"PATH": str(fake_dir) + os.pathsep + os.environ["PATH"],
                       "GIT_DIR": str(base / "forged.git"),
                       "GIT_INDEX_FILE": str(base / "forged-index"),
                       "GIT_NO_REPLACE_OBJECTS": "0"}
            with patch.dict(os.environ, hostile):
                with self.assertRaises(gate.Refused):
                    gate.verify_history(nonrepo)
            repo = base / "genuine"
            repo.mkdir()
            env = gate._git_env()
            for args in (("init", "-q"), ("config", "user.name", "Task18 Test"),
                         ("config", "user.email", "task18@example.invalid")):
                subprocess.run([str(system_git), "-C", str(repo), *args], check=True,
                               capture_output=True, env=env)
            (repo / "source").write_text("genuine")
            for args in (("add", "source"), ("commit", "-qm", "candidate")):
                subprocess.run([str(system_git), "-C", str(repo), *args], check=True,
                               capture_output=True, env=env)
            head = subprocess.check_output([str(system_git), "-C", str(repo),
                                            "rev-parse", "HEAD"], env=env, text=True).strip()
            with patch.dict(os.environ, hostile), patch.object(gate, "INTEGRATION_COMMIT", head):
                self.assertEqual(gate.verify_history(repo), head)

    def test_core_worktree_cannot_redirect_history_to_clean_sibling(self) -> None:
        system_git = Path('/usr/bin/git')
        if not system_git.is_file():
            self.skipTest('fixed system Git unavailable')
        with tempfile.TemporaryDirectory(prefix='task18-worktree-authority-') as directory:
            base = Path(directory).resolve()
            selected = base / 'selected'; selected.mkdir()
            sibling = base / 'sibling'; sibling.mkdir()
            env = gate._git_env()
            def git(*args: str) -> str:
                return subprocess.check_output([str(system_git), '-C', str(selected), *args],
                                               env=env, text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'Task18 Test')
            git('config', 'user.email', 'task18@example.invalid')
            (selected / 'source').write_text('clean\n')
            git('add', 'source')
            git('commit', '-qm', 'selected source')
            head = git('rev-parse', 'HEAD')
            (sibling / 'source').write_text('clean\n')
            (selected / 'source').write_text('dirty\n')
            git('config', 'core.worktree', str(sibling))
            self.assertEqual(git('status', '--porcelain=v1', '--untracked-files=all'), '')
            self.assertEqual(git('rev-parse', '--show-toplevel'), str(sibling))
            with patch.object(gate, 'INTEGRATION_COMMIT', head):
                with self.assertRaisesRegex(gate.Refused, 'worktree|root'):
                    gate.verify_history(selected)

    def test_replace_ref_cannot_supply_candidate_ancestry(self) -> None:
        with tempfile.TemporaryDirectory(prefix="task18-replace-") as directory:
            root = Path(directory)
            env = dict(os.environ, GIT_NO_REPLACE_OBJECTS="1")
            def git(*args: str) -> str:
                return subprocess.check_output(["git", "-C", str(root), *args],
                                               text=True, env=env).strip()
            git("init", "-q")
            git("config", "user.name", "Task18 Test")
            git("config", "user.email", "task18@example.invalid")
            (root / "source").write_text("integration")
            git("add", "source")
            git("commit", "-qm", "integration")
            integration = git("rev-parse", "HEAD")
            git("checkout", "-q", "--orphan", "candidate")
            git("rm", "-qf", "source")
            (root / "source").write_text("candidate")
            git("add", "source")
            git("commit", "-qm", "candidate")
            head = git("rev-parse", "HEAD")
            replacement = git("commit-tree", git("rev-parse", "HEAD^{tree}"),
                              "-p", integration, "-m", "replacement")
            git("replace", head, replacement)
            self.assertEqual(git("rev-parse", "HEAD"), head)
            with patch.object(gate, "INTEGRATION_COMMIT", integration):
                with self.assertRaisesRegex(gate.Refused, "lineage"):
                    gate.verify_history(root)

    def test_replace_ref_cannot_supply_candidate_tree(self) -> None:
        with tempfile.TemporaryDirectory(prefix="task18-replace-tree-") as directory:
            root = Path(directory)
            env = dict(os.environ, GIT_NO_REPLACE_OBJECTS="1")
            def git(*args: str) -> str:
                return subprocess.check_output(["git", "-C", str(root), *args],
                                               text=True, env=env).strip()
            git("init", "-q")
            git("config", "user.name", "Task18 Test")
            git("config", "user.email", "task18@example.invalid")
            (root / "source").write_text("integration")
            git("add", "source")
            git("commit", "-qm", "integration")
            integration = git("rev-parse", "HEAD")
            (root / "source").write_text("original candidate")
            git("add", "source")
            git("commit", "-qm", "candidate")
            head = git("rev-parse", "HEAD")
            (root / "source").write_text("substituted candidate")
            git("add", "source")
            replacement_tree = git("write-tree")
            git("reset", "--hard", head)
            replacement = git("commit-tree", replacement_tree, "-p", integration,
                              "-m", "replacement")
            git("replace", head, replacement)
            subprocess.run(["git", "-C", str(root), "reset", "--hard", "HEAD"],
                           check=True, capture_output=True)
            self.assertEqual((root / "source").read_text(), "substituted candidate")
            with patch.object(gate, "INTEGRATION_COMMIT", integration):
                with self.assertRaisesRegex(gate.Refused, "clean|index"):
                    gate.verify_history(root)


class SignedEvidenceControls(unittest.TestCase):
    def test_ambient_path_verifier_cannot_accept_forged_signature(self) -> None:
        verifier = Path("/usr/bin/ssh-keygen")
        if not verifier.is_file():
            self.skipTest("fixed system SSH verifier unavailable")
        with tempfile.TemporaryDirectory(prefix="task18-fake-verifier-") as directory:
            root = Path(directory).resolve()
            fake = root / "ssh-keygen"
            fake.write_text("#!/bin/sh\nexit 0\n")
            fake.chmod(0o755)
            payload = root / "appendix.json"
            payload.write_text('{"schema":"forged"}')
            signature = root / "appendix.json.sig"
            signature.write_text("forged")
            binding = {"path": str(payload), "sha256": gate._sha(payload.read_bytes()),
                       "signature_path": str(signature)}
            with patch.dict(os.environ, {"PATH": str(root) + os.pathsep + os.environ["PATH"]}):
                with self.assertRaisesRegex(gate.Refused, "signature invalid"):
                    gate._signed(binding, root, b"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAItest",
                                 test_only_temporary_evidence=True)
                private = root / "test-only-controller"
                subprocess.run([str(verifier), "-q", "-t", "ed25519", "-N", "", "-f", str(private)],
                               check=True, capture_output=True)
                public = private.with_suffix(".pub").read_bytes()
                signature.unlink()
                payload.write_text('{"schema":"valid"}')
                binding["sha256"] = gate._sha(payload.read_bytes())
                subprocess.run([str(verifier), "-Y", "sign", "-f", str(private), "-n",
                                "oxidex-task18-controller", str(payload)], check=True, capture_output=True)
                self.assertEqual(gate._signed(binding, root, public,
                                 test_only_temporary_evidence=True), {"schema": "valid"})

    def test_signed_json_rejects_duplicate_keys_and_non_json_constants(self) -> None:
        verifier = Path("/usr/bin/ssh-keygen")
        if not verifier.is_file():
            self.skipTest("fixed system SSH verifier unavailable")
        with tempfile.TemporaryDirectory(prefix="task18-signed-json-") as directory:
            root = Path(directory).resolve()
            private = root / "test-only-controller"
            subprocess.run([str(verifier), "-q", "-t", "ed25519", "-N", "", "-f", str(private)],
                           check=True, capture_output=True)
            public = private.with_suffix(".pub").read_bytes()
            for name, raw in (("duplicate", b'{"schema":"first","schema":"second"}'),
                              ("nested-duplicate", b'{"approval":{"status":"first","status":"second"}}'),
                              ("constant", b'{"schema":"valid","count":NaN}')):
                with self.subTest(name=name):
                    payload = root / f"{name}.json"
                    payload.write_bytes(raw)
                    subprocess.run([str(verifier), "-Y", "sign", "-f", str(private), "-n",
                                    "oxidex-task18-controller", str(payload)], check=True, capture_output=True)
                    binding = {"path": str(payload), "sha256": gate._sha(raw),
                               "signature_path": str(payload) + ".sig"}
                    with self.assertRaisesRegex(gate.Refused, "signed evidence JSON"):
                        gate._signed(binding, root, public, test_only_temporary_evidence=True)


class ProspectiveAuthenticatedPacketControls(unittest.TestCase):
    def test_signed_packet_acceptance_and_mutations(self) -> None:
        if not shutil.which("ssh-keygen"):
            self.skipTest("ssh-keygen unavailable")
        with tempfile.TemporaryDirectory(prefix="task18-packet-") as directory:
            ops = Path(directory).resolve()
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
                # Test-only custody seam: exercise the subsequent attribution
                # refusal without treating this temporary root as production.
                with patch.object(gate.ops_paths, "durable_root", return_value=ops):
                    with self.assertRaisesRegex(gate.Refused, "BLOCKED_ATTRIBUTION"):
                        gate.evaluate_prospective(packet, root=ROOT, ops_root=ops,
                                                 binary_path=binary_file, controller_key=trusted)
                with self.assertRaisesRegex(gate.Refused, "BLOCKED_AUTHORITY"):
                    gate.evaluate_prospective(packet, root=ROOT, ops_root=ops,
                                             binary_path=binary_file, controller_key=None)
                for index, invalid_fixture in enumerate((None, "not-a-sha", "sha256:" + "x" * 64)):
                    altered = copy.deepcopy(packet)
                    oracle_document = json.loads(Path(receipts["oracle"]["path"]).read_text())
                    reach_document = json.loads(Path(receipts["zero_reachability"]["path"]).read_text())
                    if invalid_fixture is None:
                        oracle_document.pop("fixture_sha256")
                        reach_document.pop("fixture_sha256")
                    else:
                        oracle_document["fixture_sha256"] = invalid_fixture
                        reach_document["fixture_sha256"] = invalid_fixture
                    altered["entries"][0]["receipt_bindings"]["oracle"] = sign(f"oracle-invalid-{index}", oracle_document)
                    altered["entries"][0]["receipt_bindings"]["zero_reachability"] = sign(f"reach-invalid-{index}", reach_document)
                    manifest_document = json.loads(Path(manifest["path"]).read_text())
                    manifest_document["receipts"][symbol] = {
                        kind: binding["sha256"] for kind, binding in altered["entries"][0]["receipt_bindings"].items()}
                    altered["manifest"] = sign(f"manifest-invalid-{index}", manifest_document)
                    altered["entries"][0]["controller_reconciliation_manifest_sha256"] = altered["manifest"]["sha256"]
                    with self.subTest(invalid_fixture=invalid_fixture), self.assertRaisesRegex(gate.Refused, "fixture"):
                        gate.evaluate_prospective(altered, root=ROOT, ops_root=ops,
                                                 binary_path=binary_file, controller_key=trusted,
                                                 test_only_attribution_bridge=lambda _attr, _fields: True)


class LiteralFieldProjectionControls(unittest.TestCase):
    def test_raw_child_replay_requires_exact_field_value_and_nonzero_loss(self) -> None:
        source = json.loads((HERE / "conv_exif_main_ledger.json").read_text())
        definition = next(item for item in source["generated"] if item["id"] == "0x9c9b")
        row = field("0x9c9b", "generated", "src/exiftool_tables/conv/exif_main.rs::decode")
        row["source_sha256"] = definition["source_sha256"]
        stable = gate.ownership.StableFieldId.from_row(row).text()
        with tempfile.TemporaryDirectory(prefix="task18-field-") as directory:
            root = Path(directory)
            raw_carrier = b"II\x2a\x00" + struct.pack("<I", 8) + struct.pack("<H", 1) + struct.pack("<HHII", 0x9c9b, 1, 1, 1) + struct.pack("<I", 0)
            digest = hashlib.sha256(raw_carrier).hexdigest()
            for name in ("XP.jpg", "Declined.jpg"):
                (root / name).write_bytes(raw_carrier)
            def child(mode: str, oracle: dict, candidate: dict, carrier="XP.jpg") -> dict:
                place = root / mode
                place.mkdir()
                records = {"source_sha256": digest, "staged_sha256": digest}
                for side, value in (("oracle", oracle), ("candidate", candidate)):
                    output = place / f"{side}.stdout"
                    output.write_text(json.dumps([value]))
                    records[side] = {"stdout": {"path": str(output)}}
                process = place / "process.json"
                process.write_text(json.dumps(records))
                return {"relative_path": carrier, "process": {"path": str(process)}}
            on = {"IFD0:XPTitle": "Hi"}
            oracle = {"EXIF:IFD0:XPTitle": "Hi"}
            receipt = {"selection": {"ordered_paths": ["XP.jpg"], "ordered_manifest": [
                {"relative_path": "XP.jpg", "staged_path": str(root / "XP.jpg"), "sha256": digest}]}, "runs": {
                "control-empty": {"children": [child("on", oracle, on)]},
                "engine": {"children": [child("off", oracle, {})]}}}
            occurrence = field_projection.attribute.occurrence_sequence(on, normalize_access_date=False)
            spec = {"source_field": stable, "stable_field_id": stable, "owner": row["symbol"],
                    "carrier": "XP.jpg", "token": "engine", "oracle_key": "EXIF:IFD0:XPTitle",
                    "candidate_key": "IFD0:XPTitle", "fallback_carrier": None,
                    "on": occurrence, "off": [], "fallback_on": [], "fallback_off": []}
            self.assertEqual(field_projection.project(receipt, [spec], [stable], [row])[0]["on_count"], 1)
            residual = field("IFD0/0x9c9b", "residual",
                             "src/core/exif_dir_engine.rs::IFD0_HAND_ON_DECLINE",
                             disposition="fallback-on-decline")
            with self.assertRaisesRegex(field_projection.BlockedAttribution, "fallback"):
                field_projection.project(receipt, [spec], [stable], [row, residual])
            receipt["selection"]["ordered_paths"].append("Declined.jpg")
            receipt["selection"]["ordered_manifest"].append(
                {"relative_path": "Declined.jpg", "staged_path": str(root / "Declined.jpg"), "sha256": digest})
            for mode, label in (("control-empty", "fallback-on"), ("engine", "fallback-off")):
                item = child(label, oracle, on, "Declined.jpg")
                receipt["runs"][mode]["children"].append(item)
            spec["fallback_carrier"] = "Declined.jpg"
            spec["fallback_on"] = occurrence
            spec["fallback_off"] = occurrence
            self.assertEqual(field_projection.project(receipt, [spec], [stable], [row, residual])[0]["fallback_count"], 1)
            changed = copy.deepcopy(spec)
            changed["on"][0]["value"] = "invented"
            with self.assertRaisesRegex(field_projection.BlockedAttribution, "typed ordered"):
                field_projection.project(receipt, [changed], [stable], [row, residual])
            unchanged = child("unchanged", oracle, on)
            receipt["runs"]["engine"]["children"][0] = unchanged
            spec["off"] = occurrence
            with self.assertRaisesRegex(field_projection.BlockedAttribution, "UNEXERCISED"):
                field_projection.project(receipt, [spec], [stable], [row, residual])


if __name__ == "__main__":
    unittest.main()
