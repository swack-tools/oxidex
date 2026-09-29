"""Small real-Git controls for the generated read measurement snapshot."""
from __future__ import annotations

import os
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import version_rehearsal_clean_snapshot as snapshot
import version_rehearsal_executor as executor
import version_transition_qualification as qualification


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


class CleanSnapshotTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.owned = self.root / "owned"
        self.owned.mkdir()
        self.target = self.root / "target"
        self.target.mkdir()
        subprocess.run(["git", "init", "-q", str(self.owned)], check=True)
        self.key = self.root / "signing-key"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(self.key)],
                       check=True, capture_output=True)
        self.allowed_signers = self.root / "allowed-signers"
        self.allowed_signers.write_text(f"test@example.invalid {self.key.with_suffix('.pub').read_text().strip()}\n")
        self.git_config = self.root / ".gitconfig"
        self.git_config.write_text(
            "[user]\n\tname = Test Signer\n\temail = test@example.invalid\n"
            "[gpg]\n\tformat = ssh\n"
            f"[user]\n\tsigningkey = {self.key.with_suffix('.pub')}\n"
            f"[gpg \"ssh\"]\n\tallowedSignersFile = {self.allowed_signers}\n"
        )
        env = patch.dict(os.environ, {"HOME": str(self.root)})
        env.start()
        self.addCleanup(env.stop)
        (self.owned / ".exiftool-version").write_text("13.59\n")
        (self.owned / "generated.txt").write_text("old\n")
        (self.owned / "ordinary.py").write_text("print('unchanged')\n")
        git(self.owned, "add", "-A")
        git(self.owned, "commit", "-q", "-m", "base")
        self.parent = git(self.owned, "rev-parse", "HEAD")
        (self.owned / "generated.txt").write_text("new\n")

    def create(self) -> dict:
        return snapshot.create(
            self.owned, self.target, self.parent,
            snapshot.source_tree_sha256(self.owned),
            {"generated.txt", ".exiftool-version"},
        )

    def test_signed_clean_child_has_exact_generated_bytes_and_preserves_owned_checkout(self) -> None:
        before = snapshot.source_tree_sha256(self.owned)
        proof = self.create()
        measured = Path(proof["path"])
        self.assertEqual(proof["parent_commit"], self.parent)
        self.assertEqual(proof["source_tree_sha256"], before)
        self.assertEqual(git(measured, "rev-parse", "HEAD^"), self.parent)
        self.assertEqual(git(measured, "status", "--porcelain=v1"), "")
        self.assertEqual(git(self.owned, "rev-parse", "HEAD"), self.parent)
        self.assertEqual((self.owned / "generated.txt").read_bytes(), (measured / "generated.txt").read_bytes())
        snapshot.validate(proof, self.owned, self.target, self.parent, before,
                          {"generated.txt", ".exiftool-version"})

    def test_unexpected_untracked_and_symlink_inputs_refuse(self) -> None:
        (self.owned / "unexpected.txt").write_text("unsafe")
        with self.assertRaisesRegex(snapshot.Refused, "untracked"):
            self.create()
        (self.owned / "unexpected.txt").unlink()
        (self.owned / "ordinary.py").unlink()
        (self.owned / "ordinary.py").symlink_to("generated.txt")
        with self.assertRaisesRegex(snapshot.Refused, "symlink"):
            self.create()

    def test_unrelated_tracked_change_refuses(self) -> None:
        (self.owned / "ordinary.py").write_text("print('changed')\n")
        with self.assertRaisesRegex(snapshot.Refused, "non-generated"):
            self.create()

    def test_staged_generated_change_refuses(self) -> None:
        git(self.owned, "add", "generated.txt")
        with self.assertRaisesRegex(snapshot.Refused, "index is staged"):
            self.create()

    def test_tampered_snapshot_source_parent_and_proof_refuse(self) -> None:
        proof = self.create()
        measured = Path(proof["path"])
        (measured / "generated.txt").write_text("tampered\n")
        with self.assertRaises(snapshot.Refused):
            snapshot.validate(proof, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})
        shutil.copy2(self.owned / "generated.txt", measured / "generated.txt")
        wrong = dict(proof, parent_commit="f" * 40)
        with self.assertRaises(snapshot.Refused):
            snapshot.validate(wrong, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})

    def test_signed_merge_with_identical_bytes_is_not_a_child_snapshot(self) -> None:
        proof = self.create()
        measured = Path(proof["path"])
        merged = subprocess.run(
            ["git", "-C", str(measured), "commit-tree", "-S", proof["tree"],
             "-p", self.parent, "-p", proof["commit"]], input="merge with same bytes\n",
            text=True, capture_output=True, check=True).stdout.strip()
        git(measured, "update-ref", "HEAD", merged)
        paths, patch_sha = snapshot._diff(measured, self.parent, merged)
        forged = dict(proof, commit=merged, changed_paths=paths, patch_sha256=patch_sha)
        self.assertEqual(snapshot.source_tree_sha256(measured), proof["source_tree_sha256"])
        with self.assertRaisesRegex(snapshot.Refused, "commit or checkout changed"):
            snapshot.validate(forged, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})

    def test_executor_replays_snapshot_and_rejects_rehashed_conformance_tamper(self) -> None:
        proof = self.create()
        report_path = self.root / "conformance.json"
        report = {"instrument": {
            "repo": {"root": proof["path"], "commit": proof["commit"], "tree": proof["tree"],
                     "dirty": False, "dirty_files": [], "dirty_overridden": False},
            "binary": {"path": "/authenticated/oxidex", "sha256": "a" * 64}}}
        report_path.write_text(json.dumps(report))
        result = {"measurement_snapshot": proof, "conformance_report": {
            "path": str(report_path), "sha256": hashlib.sha256(report_path.read_bytes()).hexdigest()},
            "binary": {"path": "/authenticated/oxidex", "sha256": "a" * 64}}
        generation = {"clean_source_before": {"artifact_paths": ["generated.txt"]}}
        source_tree = {"sha256": proof["source_tree_sha256"]}
        with patch.object(executor.artifacts, "inventory", return_value=[]):
            executor._require_read_measurement_snapshot(
                result, generation, self.owned, self.target, self.parent, source_tree)
            report["instrument"]["repo"]["commit"] = "f" * 40
            report_path.write_text(json.dumps(report))
            result["conformance_report"]["sha256"] = hashlib.sha256(report_path.read_bytes()).hexdigest()
            with self.assertRaisesRegex(executor.Refused, "not bound to the signed source"):
                executor._require_read_measurement_snapshot(
                    result, generation, self.owned, self.target, self.parent, source_tree)
            with self.assertRaisesRegex(executor.Refused, "lacks signed clean"):
                executor._require_read_measurement_snapshot(
                    {key: value for key, value in result.items() if key != "measurement_snapshot"},
                    generation, self.owned, self.target, self.parent, source_tree)

    def test_committed_result_side_replays_underlying_signed_source(self) -> None:
        run_dir = self.root / "row" / "before"
        checkout = run_dir / "checkouts" / executor._safe_name("13.59")
        checkout.parent.mkdir(parents=True)
        shutil.move(self.owned, checkout)
        self.owned = checkout
        proof = self.create()
        conformance = self.root / "conformance.json"
        data = {"instrument": {
            "repo": {"root": proof["path"], "commit": proof["commit"], "tree": proof["tree"],
                     "dirty": False, "dirty_files": [], "dirty_overridden": False},
            "binary": {"path": "/authenticated/oxidex", "sha256": "a" * 64}}}
        conformance.write_text(json.dumps(data))
        generation = {"clean_source_before": {"artifact_paths": ["generated.txt"]}}
        binary = self.target / "debug" / "oxidex"
        binary.parent.mkdir()
        binary.write_bytes(b"authenticated")
        binary_row = {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                      "bytes": binary.stat().st_size}
        native_source = self.root / "native"
        native_lib = native_source / "lib"
        native_pm = native_lib / "Image" / "ExifTool.pm"
        native_pm.parent.mkdir(parents=True)
        native_pm.write_text("native")
        native_module = native_pm.parent / "Canon.pm"
        native_module.write_text("native module")
        native_module_sha = hashlib.sha256(native_module.read_bytes()).hexdigest()
        native_perl = self.root / "perl"
        native_perl.write_text("perl")
        native_identity = {"release": "13.59",
                           "source": {"path": str(native_source.resolve())},
                           "lib": {"path": str(native_lib.resolve()),
                                   "exiftool_pm_sha256": hashlib.sha256(native_pm.read_bytes()).hexdigest()},
                           "perl": {"path": str(native_perl.resolve()),
                                    "sha256": hashlib.sha256(native_perl.read_bytes()).hexdigest()}}
        data["instrument"]["binary"] = {"path": str(binary), "sha256": binary_row["sha256"]}
        conformance.write_text(json.dumps(data))
        source_row = {"source_commit": self.parent, "source_tree_sha256": proof["source_tree_sha256"],
                      "generated_artifacts": [], "binary": binary_row,
                      "native_identity": native_identity}
        native_probe = {"probe_sha256": "b" * 64}
        fixture = self.root / "t" / "images" / "fixture.jpg"
        fixture.parent.mkdir(parents=True)
        fixture.write_bytes(b"fixture")
        staged = self.target / "fixture.jpg"
        staged.write_bytes(fixture.read_bytes())
        fixture_sha = hashlib.sha256(fixture.read_bytes()).hexdigest()
        fixture_manifest = self.root / "fixture-manifest.json"
        fixture_manifest.write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest",
            "fixtures": [{"path": str(fixture), "sha256": fixture_sha,
                          "bytes": fixture.stat().st_size}],
        }))
        original = {"read_manifest": str(fixture_manifest),
                    "read_binding": executor._fixture_binding(
                        str(fixture_manifest),
                        kind="oxidex_version_rehearsal_fixture_manifest", jpeg_only=False)}
        union = qualification._freeze_read_union(original, original)
        read_union = qualification._materialize_read_union(union, original, original,
                                                            self.root / "row")
        read = {"state": "measured", "acceptance": "pending_pair_policy",
                "measurement_snapshot": proof, "conformance_report": {
            "path": str(conformance), "sha256": hashlib.sha256(conformance.read_bytes()).hexdigest()},
            **source_row,
            "native_probe_sha256": native_probe["probe_sha256"],
            "fixtures": {"manifest": read_union["manifest"]["path"],
                         "manifest_sha256": read_union["manifest"]["sha256"],
                         "entries": [{"source": str(fixture), "sha256": fixture_sha,
                                      "bytes": fixture.stat().st_size, "corpus_path": str(staged),
                                      "corpus_sha256": fixture_sha, "corpus_bytes": staged.stat().st_size}]}}
        build = dict(source_row)
        reports = run_dir / "reports"
        reports.mkdir(parents=True)
        generate_path, read_path = reports / "generate.json", reports / "read.json"
        build_path = reports / "build.json"
        native_path = reports / "native.json"
        config_path = run_dir / "inputs" / "config.json"
        config_path.parent.mkdir(parents=True)
        bundle = self.root / "verified-input-bundle"
        bundle.mkdir()
        archive_cache = self.root / "archive-cache"
        archive_cache.mkdir()
        (bundle / "locations.json").write_text(json.dumps({
            "kind": "oxidex_version_transition_input_locations",
            "archive_cache": str(archive_cache), "source_root": str(native_source),
        }))
        config = {"execution_source_commit": self.parent,
                  "target_directories": {"13.59": str(self.target)},
                  "verified_input_bundle": str(bundle)}
        config_path.write_text(json.dumps(config))
        journal_path = run_dir / "execution-status.json"

        def update_outer_receipts() -> dict:
            generate_path.write_text(json.dumps(generation))
            build_path.write_text(json.dumps(build))
            native_path.write_text(json.dumps(native_probe))
            read_path.write_text(json.dumps(read))
            journal = {"config_sha256": qualification.rehearsal.sha256_json(config),
                       "phase": "complete",
                       "scope": {"read_acceptance": "pending_pair_policy",
                                 "write_acceptance": "passed_per_release"},
                       "releases": {"13.59": {"state": "measured_pending_pair_policy",
                                                "stages": {"read": "measured"}, "reports": {
                           "generate": {"path": "reports/generate.json",
                                        "sha256": qualification.rehearsal.sha256_json(generation)},
                           "build": {"path": "reports/build.json",
                                     "sha256": qualification.rehearsal.sha256_json(build)},
                           "native": {"path": "reports/native.json",
                                      "sha256": qualification.rehearsal.sha256_json(native_probe)},
                           "read": {"path": "reports/read.json",
                                    "sha256": qualification.rehearsal.sha256_json(read),
                                    "acceptance": "pending_pair_policy"}}}}}
            journal_path.write_text(json.dumps(journal))
            return {"id": "row", "read_union": read_union,
                    "before": {"release": "13.59",
                    "instrument": {"source_commit": self.parent, "native_identity": native_identity,
                                   "binary": binary_row,
                                   "native_probe_sha256": native_probe["probe_sha256"],
                    "read_fixture_manifest": read_union["manifest"]["path"],
                    "read_fixture_manifest_sha256": read_union["manifest"]["sha256"],
                                   "read_fixture_count": 1},
                    "read_report_sha256": qualification.rehearsal.sha256_json(read),
                    "execution_journal_sha256": qualification._sha_file(journal_path)}}

        row = update_outer_receipts()
        self.assertEqual(executor._source_tree(checkout)["sha256"], proof["source_tree_sha256"])
        snapshot.validate(proof, checkout, self.target, self.parent,
                          proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})
        def verify_materialization(actual_run_dir: Path, actual_cache: Path,
                                   actual_source_root: Path) -> tuple:
            self.assertEqual((actual_run_dir, actual_cache, actual_source_root),
                             (run_dir, archive_cache, native_source))
            if hashlib.sha256(native_module.read_bytes()).hexdigest() != native_module_sha:
                raise executor.Refused("native module bytes changed")
            return ()

        with (patch.object(executor.artifacts, "inventory", return_value=[]),
              patch.object(executor, "_verify_inputs", side_effect=verify_materialization),
              patch.object(executor, "_require_read_counts") as read_counts,
              patch.object(executor, "_require_read_raw_maps") as raw_maps):
            reread = qualification._report_for(run_dir, json.loads(journal_path.read_text()),
                                               "13.59", "read")
            self.assertEqual(reread["measurement_snapshot"], proof)
            qualification._replay_committed_read_snapshot(row, "before", self.root)
            read_counts.assert_called_once_with(read)
            self.assertEqual(raw_maps.call_args.args[0], read)
            self.assertEqual(raw_maps.call_args.args[1], read_path)
            native_module.write_text("tampered native module")
            with self.assertRaisesRegex(qualification.Refused, "materialized native source changed"):
                qualification._replay_committed_read_snapshot(row, "before", self.root)
            native_module.write_text("native module")
            binary.write_bytes(b"changed")
            with self.assertRaisesRegex(qualification.Refused, "read measurement replay refused"):
                qualification._replay_committed_read_snapshot(row, "before", self.root)
            binary.write_bytes(b"authenticated")
            data["instrument"]["repo"]["commit"] = "f" * 40
            conformance.write_text(json.dumps(data))
            read["conformance_report"]["sha256"] = hashlib.sha256(conformance.read_bytes()).hexdigest()
            row = update_outer_receipts()
            with self.assertRaisesRegex(qualification.Refused, "read measurement replay refused"):
                qualification._replay_committed_read_snapshot(row, "before", self.root)
        wrong = dict(proof, commit="f" * 40)
        with self.assertRaises(snapshot.Refused):
            snapshot.validate(wrong, self.owned, self.target, self.parent,
                              proof["source_tree_sha256"], {"generated.txt", ".exiftool-version"})


if __name__ == "__main__":
    unittest.main()
