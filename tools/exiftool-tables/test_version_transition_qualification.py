#!/usr/bin/env python3
"""Contracts for the non-promoting Task19 transition wrapper."""

from __future__ import annotations

from contextlib import redirect_stderr
import io
import json
import os
import signal
import subprocess
import sys
import threading
import time
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from pathlib import Path


HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import version_transition_qualification as qualification
import test_version_rehearsal_native_oracle as native_fixture


class MatrixContractTests(unittest.TestCase):
    def test_checked_matrix_has_all_required_rows_and_dynamic_same_pin(self) -> None:
        matrix = qualification.load_matrix(HERE / "version_transition_matrix.json", "13.59")
        self.assertEqual(
            [row["id"] for row in matrix["rows"]],
            ["same-pin-13.59", "11.78-to-12.64", "12.64-to-11.78"],
        )
        self.assertEqual(matrix["rows"][0]["before_version"], "13.59")
        self.assertEqual(matrix["rows"][0]["after_version"], "13.59")

    def test_every_row_binds_sources_fixtures_artifacts_targets_and_write(self) -> None:
        matrix = qualification.load_matrix(HERE / "version_transition_matrix.json", "13.60")
        self.assertEqual(len({row["target_directory"] for row in matrix["rows"]}), 3)
        self.assertEqual(len({row["durable_output_directory"] for row in matrix["rows"]}), 3)
        for row in matrix["rows"]:
            self.assertEqual(row["fresh_generation"], "both-sides")
            self.assertEqual(row["native_read"], "mandatory")
            self.assertEqual(row["native_write_readback"], "mandatory")
            self.assertEqual(row["promotion"], "forbidden")
            self.assertEqual(set(row["immutable_source_identities"]), {"before", "after"})
            for side in ("before", "after"):
                self.assertEqual(row["immutable_source_identities"][side]["resolver"], "verified-input-bundle")
                self.assertEqual(set(row["fixtures"][side]),
                                 {"read_manifest", "write_manifest", "native_cases"})
            self.assertEqual(row["artifact_manifest"]["resolver"], "live-generated-inventory")

    def test_missing_mandatory_write_contract_is_refused(self) -> None:
        raw = json.loads((HERE / "version_transition_matrix.json").read_text())
        raw["rows"][1]["native_write_readback"] = "optional"
        with TemporaryDirectory() as temporary:
            candidate = Path(temporary) / "matrix.json"
            candidate.write_text(json.dumps(raw))
            with self.assertRaisesRegex(qualification.Refused, "mandatory"):
                qualification.load_matrix(candidate, "13.59")

    def test_row_labels_bind_exact_versions_policies_and_fixed_source_commits(self) -> None:
        original = json.loads((HERE / "version_transition_matrix.json").read_text())
        mutations = (
            (1, "before_version", "12.00"),
            (1, "artifact_manifest", {**original["rows"][1]["artifact_manifest"], "comparison": "identical"}),
            (2, "artifact_manifest", {**original["rows"][2]["artifact_manifest"], "comparison": "manifest-delta"}),
        )
        for index, key, value in mutations:
            with self.subTest(index=index, key=key), TemporaryDirectory() as temporary:
                changed = json.loads(json.dumps(original))
                changed["rows"][index][key] = value
                candidate = Path(temporary) / "matrix.json"
                candidate.write_text(json.dumps(changed))
                with self.assertRaisesRegex(qualification.Refused, "Task19 contract|row label"):
                    qualification.load_matrix(candidate, "13.59")
        for side in qualification.SIDES:
            with self.subTest(side=side), TemporaryDirectory() as temporary:
                changed = json.loads(json.dumps(original))
                changed["rows"][1]["immutable_source_identities"][side].pop("expected_peeled_commit")
                candidate = Path(temporary) / "matrix.json"
                candidate.write_text(json.dumps(changed))
                with self.assertRaisesRegex(qualification.Refused, "fixed source identity"):
                    qualification.load_matrix(candidate, "13.59")


class VerifiedInputTests(unittest.TestCase):
    def test_source_identity_uses_real_catalog_and_materialization_verifiers(self) -> None:
        temporary, capture, catalog, plan, resolution, materialization, cache, sources, release = native_fixture.make_state()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        bundle = root / "bundle"
        bundle.mkdir()
        for name, document in zip(qualification.INPUT_NAMES,
                                  (capture, catalog, plan, resolution, materialization), strict=True):
            (bundle / f"{name}.json").write_text(json.dumps(document))
        (bundle / "locations.json").write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_transition_input_locations",
            "archive_cache": str(cache), "source_root": str(sources),
        }))
        side = next(side for pair in plan["pairs"] for side in (pair["old"], pair["new"])
                    if side["release"] == release)
        identity = qualification.resolve_source_identity(
            {"expected_release": release, "expected_peeled_commit": side["peeled_commit"]}, bundle,
        )
        self.assertEqual(identity["release"], release)
        self.assertEqual(identity["peeled_commit"], side["peeled_commit"])
        self.assertRegex(identity["source_tree_sha256"], r"^[0-9a-f]{64}$")

        changed = dict(side)
        changed["peeled_commit"] = "0" * 40
        with self.assertRaisesRegex(qualification.Refused, "checked expectation"):
            qualification.resolve_source_identity(
                {"expected_release": release, "expected_peeled_commit": changed["peeled_commit"]}, bundle,
            )


class CallerRestorationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.repository = Path(self.temporary.name) / "repo"
        self.repository.mkdir()
        subprocess.run(["git", "init", "-q", str(self.repository)], check=True)
        subprocess.run(["git", "-C", str(self.repository), "config", "user.name", "Test"], check=True)
        subprocess.run(["git", "-C", str(self.repository), "config", "user.email", "test@example.invalid"], check=True)
        (self.repository / ".exiftool-version").write_text("13.59\n")
        (self.repository / "tracked").write_text("clean\n")
        subprocess.run(["git", "-C", str(self.repository), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.repository), "commit", "-qm", "fixture"], check=True)

    def test_clean_snapshot_verifies_and_pin_or_index_tampering_refuses(self) -> None:
        with patch.object(qualification.artifacts, "inventory", return_value=[]):
            snapshot = qualification.snapshot_caller(self.repository)
            qualification.verify_caller(snapshot)
            (self.repository / ".exiftool-version").write_text("12.64\n")
            with self.assertRaisesRegex(qualification.Refused, "clean"):
                qualification.verify_caller(snapshot)
            subprocess.run(["git", "-C", str(self.repository), "checkout", "--", ".exiftool-version"], check=True)
            (self.repository / "tracked").write_text("staged\n")
            subprocess.run(["git", "-C", str(self.repository), "add", "tracked"], check=True)
            with self.assertRaisesRegex(qualification.Refused, "clean"):
                qualification.verify_caller(snapshot)

    def test_dirty_caller_is_refused_before_work(self) -> None:
        (self.repository / "untracked").write_text("no")
        with patch.object(qualification.artifacts, "inventory", return_value=[]):
            with self.assertRaisesRegex(qualification.Refused, "clean"):
                qualification.snapshot_caller(self.repository)


class SideAndRecoveryTests(unittest.TestCase):
    def test_side_config_selects_one_release_and_mandatory_write(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture = root / "fixture.jpg"
            fixture.write_bytes(b"\xff\xd8fixture")
            manifest = root / "write.json"
            manifest.write_text(json.dumps({
                "schema": 1,
                "kind": "oxidex_version_rehearsal_write_fixture_manifest",
                "fixtures": [{"path": str(fixture), "sha256": qualification._sha_file(fixture),
                              "bytes": fixture.stat().st_size}],
            }))
            read_manifest = root / "read.json"
            read_manifest.write_text(json.dumps({
                "schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest",
                "fixtures": [{"path": str(fixture), "sha256": qualification._sha_file(fixture),
                              "bytes": fixture.stat().st_size}],
            }))
            config = qualification._side_config(
                release="11.78", source_commit="a" * 40, perl=Path(sys.executable).resolve(),
                read_manifest=read_manifest, write_manifest=manifest,
                native_cases=[{"name": "case"}], lease=root / "lease", target=root / "target",
            )
            normalized = qualification.executor._config(config, ["11.78", "12.64"])
            self.assertEqual(normalized["execution_releases"], ["11.78"])
            self.assertIn("write", normalized["commands"])
            self.assertEqual(set(normalized["write_fixture_bindings"]), {"11.78"})

    def test_same_pin_tampering_and_reverse_without_removal_are_refused(self) -> None:
        artifact = [{"path": "generated", "sha256": "a" * 64, "bytes": 1}]
        side = {"generated_artifacts": artifact}
        qualification._compare_sides(
            {"artifact_manifest": {"comparison": "identical"}}, side, side,
        )
        changed = {"generated_artifacts": [{"path": "generated", "sha256": "b" * 64, "bytes": 1}]}
        with self.assertRaisesRegex(qualification.Refused, "different"):
            qualification._compare_sides(
                {"artifact_manifest": {"comparison": "identical"}}, side, changed,
            )
        with self.assertRaisesRegex(qualification.Refused, "removed"):
            qualification._compare_sides(
                {"artifact_manifest": {"comparison": "manifest-delta-with-removals"}}, side, side,
            )

    def test_running_execution_is_recovered_but_terminal_is_not_reused(self) -> None:
        with TemporaryDirectory() as temporary:
            run_dir = Path(temporary) / "run"
            run_dir.mkdir()
            journal = {"phase": "running", "active": {"release": "11.78", "stage": "generate"}}
            (run_dir / "execution-status.json").write_text(json.dumps(journal))
            with patch.object(qualification.executor, "recover") as recover:
                qualification._recover_if_running(run_dir, Path("cache"), Path("sources"), host_lock_fd=17)
            recover.assert_called_once_with(run_dir, Path("cache"), Path("sources"), host_lock_fd=17)
            journal["phase"] = "interrupted"
            (run_dir / "execution-status.json").write_text(json.dumps(journal))
            with patch.object(qualification.executor, "recover") as recover:
                qualification._recover_if_running(run_dir, Path("cache"), Path("sources"))
            recover.assert_not_called()

    def test_real_recovery_reuses_the_validated_external_lease(self) -> None:
        temporary, capture, catalog, plan, resolution, materialization, cache, sources, release = native_fixture.make_state()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        fixture = root / "fixture.jpg"; fixture.write_bytes(b"\xff\xd8fixture")
        read_manifest = root / "read.json"
        read_manifest.write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest",
            "fixtures": [{"path": str(fixture), "sha256": qualification._sha_file(fixture),
                          "bytes": fixture.stat().st_size}],
        }))
        write_manifest = root / "write.json"
        write_manifest.write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_write_fixture_manifest",
            "fixtures": [{"path": str(fixture), "sha256": qualification._sha_file(fixture),
                          "bytes": fixture.stat().st_size}],
        }))
        lease_path = root / "transition.host.lock"; lease_path.touch()
        run_dir = root / "run"
        config = qualification._side_config(
            release=release, source_commit=plan["repository_commit"], perl=Path(sys.executable).resolve(),
            read_manifest=read_manifest, write_manifest=write_manifest, native_cases=[{"name": "case"}],
            lease=lease_path, target=root / "target",
        )
        qualification.executor.initialize_run(
            run_dir, capture, catalog, plan, resolution, materialization, config,
        )
        journal_path = run_dir / "execution-status.json"
        journal = json.loads(journal_path.read_text())
        journal["phase"] = "running"
        journal["active"] = {"release": release, "stage": "generate"}
        journal["releases"][release]["stages"]["generate"] = "running"
        journal_path.write_text(json.dumps(journal))
        receipts = [root / name for name in ("owner.json", "heartbeat.jsonl", "expiry.json", "release.json")]
        with qualification.TransitionLease(
            lease=lease_path, run_id="recover", owner_receipt=receipts[0],
            heartbeat_receipt=receipts[1], expiry_receipt=receipts[2], release_receipt=receipts[3],
        ) as held:
            qualification._recover_if_running(run_dir, cache, sources, host_lock_fd=held.fileno)
        self.assertEqual(json.loads(journal_path.read_text())["phase"], "interrupted")

    def test_report_for_requires_the_execution_journal_digest(self) -> None:
        with TemporaryDirectory() as temporary:
            run_dir = Path(temporary)
            report_path = run_dir / "stage-results/release-11-78/read.json"
            report_path.parent.mkdir(parents=True)
            original = {"state": "passed", "classification_counts": {"extra": 0}}
            report_path.write_text(json.dumps(original))
            journal = {"releases": {"11.78": {"reports": {"read": {
                "path": str(report_path.relative_to(run_dir)),
                "sha256": qualification.rehearsal.sha256_json(original),
            }}}}}
            report_path.write_text(json.dumps({"state": "passed", "classification_counts": {"extra": 99}}))
            with self.assertRaisesRegex(qualification.Refused, "journal digest"):
                qualification._report_for(run_dir, journal, "11.78", "read")


class LeaseTests(unittest.TestCase):
    def test_nonblocking_lease_emits_all_receipts(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lease_path = root / "transition.host.lock"
            lease_path.touch()
            receipts = [root / name for name in ("owner.json", "heartbeat.jsonl", "expiry.json", "release.json")]
            with qualification.TransitionLease(
                lease=lease_path, run_id="unit-run", owner_receipt=receipts[0],
                heartbeat_receipt=receipts[1], expiry_receipt=receipts[2], release_receipt=receipts[3],
            ) as lease:
                self.assertTrue(os.get_inheritable(lease.fileno))
                lease.heartbeat("unit", "same-pin", "before")
                lease.finish("complete")
            self.assertTrue(all(receipt.is_file() for receipt in receipts))
            self.assertEqual(json.loads(receipts[3].read_text())["release_status"], "released")
            self.assertGreaterEqual(len(receipts[1].read_text().splitlines()), 2)

    def test_inherited_descriptor_keeps_lock_until_real_child_exits(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lease_path = root / "transition.host.lock"; lease_path.touch()
            child_pid = root / "child.pid"
            helper = root / "supervisor.py"
            helper.write_text(
                "import os, subprocess, sys\nfrom pathlib import Path\n"
                f"sys.path.insert(0, {str(HERE)!r})\n"
                "import version_transition_qualification as q\n"
                f"r=Path({str(root)!r}); lease=Path({str(lease_path)!r})\n"
                "held=q.TransitionLease(lease=lease, run_id='inherit', owner_receipt=r/'owner.json', "
                "heartbeat_receipt=r/'heartbeat.jsonl', expiry_receipt=r/'expiry.json', release_receipt=r/'release.json').__enter__()\n"
                "child=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(1.2)'], close_fds=False, "
                "stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n"
                f"Path({str(child_pid)!r}).write_text(str(child.pid))\n"
                "os._exit(0)\n"
            )
            finished = subprocess.run([sys.executable, str(helper)], cwd=root, timeout=5)
            self.assertEqual(finished.returncode, 0)
            self.assertTrue(child_pid.is_file())
            with self.assertRaisesRegex(qualification.executor.Refused, "already held"):
                with qualification.executor._HostLock(lease_path):
                    pass
            deadline = time.monotonic() + 4
            while time.monotonic() < deadline:
                try:
                    with qualification.executor._HostLock(lease_path):
                        break
                except qualification.executor.Refused:
                    time.sleep(0.05)
            else:
                self.fail("inherited descriptor did not release after child exit")

    def test_receipt_failures_always_release_and_preserve_body_error(self) -> None:
        for failure_kind in ("owner", "heartbeat", "expiry"):
            with self.subTest(failure_kind=failure_kind), TemporaryDirectory() as temporary:
                root = Path(temporary)
                lease_path = root / "transition.host.lock"; lease_path.touch()
                owner, heartbeat, expiry, release = [root / name for name in
                    ("owner.json", "heartbeat.jsonl", "expiry.json", "release.json")]
                lease = qualification.TransitionLease(
                    lease=lease_path, run_id=failure_kind, owner_receipt=owner,
                    heartbeat_receipt=heartbeat, expiry_receipt=expiry, release_receipt=release,
                )
                if failure_kind == "owner":
                    original = qualification._atomic_json
                    effect = lambda target, value: (_ for _ in ()).throw(OSError("owner failed")) \
                        if target == owner else original(target, value)
                    context = patch.object(qualification, "_atomic_json", side_effect=effect)
                elif failure_kind == "heartbeat":
                    context = patch.object(qualification, "_append_jsonl", side_effect=OSError("heartbeat failed"))
                else:
                    original = qualification._atomic_json
                    effect = lambda target, value: (_ for _ in ()).throw(OSError("expiry failed")) \
                        if target == expiry else original(target, value)
                    context = patch.object(qualification, "_atomic_json", side_effect=effect)
                with context:
                    if failure_kind == "expiry":
                        with self.assertRaisesRegex(RuntimeError, "body failed"):
                            with lease:
                                raise RuntimeError("body failed")
                    else:
                        with self.assertRaisesRegex(OSError, f"{failure_kind} failed"):
                            with lease:
                                pass
                self.assertIsNone(lease.file)
                with qualification.executor._HostLock(lease_path):
                    pass


class WrapperCallTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.output = self.root / "durable-output"
        self.target = self.root / "durable-target"
        self.output.mkdir()
        self.target.mkdir()
        self.lease = self.output / "transition.host.lock"
        self.lease.touch()
        self.run_id = "unit-wrapper"
        receipt_root = self.output / self.run_id
        self.receipts = {
            "owner_receipt": receipt_root / "lease-owner.json",
            "heartbeat_receipt": receipt_root / "lease-heartbeat.jsonl",
            "expiry_receipt": receipt_root / "lease-expiry.json",
            "release_receipt": receipt_root / "lease-release.json",
            "handoff_receipt": receipt_root / "handoff.jsonl",
        }
        self.row_output = self.output / self.run_id / "same-pin-13.59"
        self.row_target = self.target / self.run_id / "same-pin-13.59"
        self.row = {
            "id": "same-pin-13.59", "before_version": "13.59", "after_version": "13.59",
            "immutable_source_identities": {
                side: {"expected_release": "13.59", "input_bundle": str(self.root / "bundle")}
                for side in qualification.SIDES
            },
            "fixtures": {
                side: {"read_manifest": str(self.root / "read.json"),
                       "write_manifest": str(self.root / "write.json"),
                       "native_cases": str(self.root / "cases.json")}
                for side in qualification.SIDES
            },
            "artifact_manifest": {"comparison": "identical"},
            "target_directory": str(self.row_target),
            "durable_output_directory": str(self.row_output),
        }
        (self.root / "read.json").write_text("{}")
        fixture = self.root / "fixture.jpg"; fixture.write_bytes(b"\xff\xd8fixture")
        fixture_row = {"path": str(fixture), "sha256": qualification._sha_file(fixture),
                       "bytes": fixture.stat().st_size}
        (self.root / "read.json").write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest", "fixtures": [fixture_row],
        }))
        (self.root / "write.json").write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_write_fixture_manifest", "fixtures": [fixture_row],
        }))
        self.native_fixture = self.root / "native-only.dat"
        self.native_fixture.write_bytes(b"native fixture")
        (self.root / "cases.json").write_text(json.dumps([{
            "name": "case", "fixture": str(self.native_fixture),
            "read": {"query": "Comment", "expectation": "native_unsupported"},
            "write": {"tag": "Comment", "operation": "delete", "readback": None},
        }]))
        self.caller = {"pin_version": "13.59", "head": "a" * 40}
        self.identity = {
            "release": "13.59", "tag_object": "b" * 40, "peeled_commit": "c" * 40,
            "source_directory": "source", "source_tree_sha256": "d" * 64,
            "materialization_sha256": "e" * 64, "bundle": str(self.root / "bundle"),
            "archive_cache": str(self.root / "cache"), "source_root": str(self.root / "sources"),
            "documents": {name: ({"repository_commit": "a" * 40} if name == "plan" else {})
                          for name in qualification.INPUT_NAMES},
        }

    def invoke(self, execute):
        configs = []
        def initialize(run_dir, _capture, _catalog, _plan, _resolution, _materialization, config):
            run_dir.mkdir(parents=True)
            configs.append(config)
        artifact = [{"path": "generated", "sha256": "f" * 64, "bytes": 1}]
        side_receipt = {
            "release": "13.59", "source_identity": {}, "generated_artifacts": artifact,
            "classification_counts": {"matched": 1, "value_diff": 0, "missing": 0,
                                      "renames": 0, "extra": 0},
            "generated_refusals": {"total": 0, "counters": []},
        }
        with patch.object(qualification, "snapshot_caller", return_value=self.caller), \
             patch.object(qualification, "verify_caller"), \
             patch.object(qualification, "load_matrix", return_value={"rows": [self.row]}), \
             patch.object(qualification, "materialize_matrix", return_value={"rows": [self.row]}), \
             patch.object(qualification, "_perl", return_value=Path(sys.executable).resolve()), \
             patch.object(qualification, "resolve_source_identity", return_value=self.identity), \
             patch.object(qualification.executor, "initialize_run", side_effect=initialize), \
             patch.object(qualification, "_side_receipt", return_value=side_receipt):
            result = qualification.run_qualification(
                matrix_path=Path("matrix"), repository=Path("repository"),
                output_root=self.output, target_root=self.target,
                lease_path=self.lease, run_id=self.run_id, execute=execute, **self.receipts,
            )
        return result, configs

    def test_wrapper_calls_real_executor_seam_twice_with_write_and_owned_lock(self) -> None:
        calls = []
        def execute(run_dir, repository, archive_cache, source_root, **kwargs):
            calls.append((run_dir, repository, archive_cache, source_root, kwargs))
            return {"phase": "complete", "scope": {"write_acceptance": "passed_per_release"}}
        result, configs = self.invoke(execute)
        self.assertEqual(result["status"], "tooling-executed-nonpromoting")
        self.assertEqual(len(calls), 2)
        self.assertTrue(all(set(call[-1]) == {"host_lock_fd", "stage_guard"}
                            and type(call[-1]["host_lock_fd"]) is int for call in calls))
        self.assertEqual([config["execution_releases"] for config in configs], [["13.59"], ["13.59"]])
        self.assertTrue(all("write" in config["commands"] for config in configs))
        self.assertEqual(configs[0]["target_directories"]["13.59"], str(self.row_target / "before"))
        self.assertEqual(configs[1]["target_directories"]["13.59"], str(self.row_target / "after"))

    def test_success_receipt_is_published_only_after_lease_cleanup(self) -> None:
        original = qualification._atomic_json
        def fails_expiry_receipt(target, value):
            if target == self.receipts["expiry_receipt"]:
                raise OSError("expiry receipt failed")
            return original(target, value)
        with patch.object(qualification, "_atomic_json", side_effect=fails_expiry_receipt):
            with self.assertRaisesRegex(qualification.Refused, "lease cleanup failed"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertFalse((self.output / self.run_id / "qualification-result.json").exists())

    def test_final_lease_expiry_refuses_success_and_releases_lock(self) -> None:
        self._check_terminal_cleanup(expire="before")

    def test_expiry_during_cleanup_receipts_refuses_success(self) -> None:
        self._check_terminal_cleanup(expire="during")

    def test_final_expiry_preserves_existing_execution_exception(self) -> None:
        self._check_terminal_cleanup(expire="before", fail=True)

    def test_successful_final_cleanup_records_complete_and_releases_lock(self) -> None:
        self._check_terminal_cleanup()

    def _check_terminal_cleanup(self, *, expire=None, fail=False) -> None:
        original_exit = qualification.TransitionLease.__exit__
        original_write = qualification._atomic_json
        captured = []
        failure = RuntimeError("execution failed before cleanup")

        def cleanup(lease, *args):
            captured.append((lease, lease.file))
            if expire == "before":
                lease.expires_at = 0
            return original_exit(lease, *args)

        def write(target, value):
            original_write(target, value)
            if expire == "during" and target == self.receipts["release_receipt"]:
                captured[0][0].expires_at = 0

        def execute(*_args, **_kwargs):
            if fail:
                raise failure
            return {"phase": "complete", "scope": {"write_acceptance": "passed_per_release"}}

        with patch.object(qualification.TransitionLease, "__exit__", cleanup), \
             patch.object(qualification, "_atomic_json", side_effect=write):
            if fail:
                with self.assertRaises(RuntimeError) as raised:
                    self.invoke(execute)
                self.assertIs(raised.exception, failure)
            elif expire:
                with self.assertRaisesRegex(qualification.Refused, "lease expired"):
                    self.invoke(execute)
            else:
                self.invoke(execute)
        self.assertEqual((self.output / self.run_id / "qualification-result.json").exists(),
                         not (expire or fail))
        expiry = json.loads(self.receipts["expiry_receipt"].read_text())
        release = json.loads(self.receipts["release_receipt"].read_text())
        self.assertEqual(expiry["expiry_status"], "expired" if expire else "not-expired")
        for receipt in (expiry, release):
            self.assertEqual(receipt["terminal_status"], "failed" if expire or fail else "complete")
        self.assertEqual(release["release_status"], "released")
        self.assertTrue(release["flock_release_confirmed"])
        self.assertIsNone(captured[0][0].file)
        self.assertTrue(captured[0][1].closed)
        with qualification.executor._HostLock(self.lease):
            pass

    def test_contending_invocation_writes_no_handoff_before_lease(self) -> None:
        with qualification.executor._HostLock(self.lease):
            with self.assertRaisesRegex(qualification.Refused, "transition lease is held"):
                self.invoke(unittest.mock.Mock())
        self.assertFalse(self.receipts["handoff_receipt"].exists())

    def test_interruption_recovers_active_executor_journal(self) -> None:
        def interrupted(run_dir, _repository, _archive_cache, _source_root, **_kwargs):
            (run_dir / "execution-status.json").write_text(json.dumps({
                "phase": "running", "active": {"release": "13.59", "stage": "generate"},
            }))
            raise KeyboardInterrupt()
        with patch.object(qualification.executor, "recover") as recover:
            with self.assertRaises(KeyboardInterrupt) as caught:
                self.invoke(interrupted)
        recover.assert_called_once()
        self.assertIsInstance(recover.call_args.kwargs["host_lock_fd"], int)
        self.assertEqual(getattr(caught.exception, "_oxidex_durable_recovery"), "recovered")
        self.assertEqual(json.loads(self.receipts["release_receipt"].read_text())["terminal_status"], "failed")

    def test_real_child_interrupt_is_reaped_before_recovery_and_lock_release(self) -> None:
        """A supervisor-only interrupt must not orphan its owned child group."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        ready_error: list[BaseException] = []

        child_program = (
            "import json, os, subprocess, sys\n"
            "descendant = subprocess.Popen([sys.executable, '-c', "
            "'import time; time.sleep(60)'])\n"
            "os.write(int(sys.argv[1]), (json.dumps({'descendant': descendant.pid}) + '\\n').encode())\n"
            "raise SystemExit(descendant.wait())\n"
        )

        def interrupt_when_child_group_is_ready() -> None:
            try:
                payload = b""
                while not payload.endswith(b"\n"):
                    chunk = os.read(read_fd, 4096)
                    if not chunk:
                        raise AssertionError("owned child closed readiness pipe before reporting its descendant")
                    payload += chunk
                process_ids.update(json.loads(payload))
                os.kill(os.getpid(), signal.SIGINT)
            except BaseException as exc:
                ready_error.append(exc)

        interrupter = threading.Thread(target=interrupt_when_child_group_is_ready, daemon=True)

        def recover(run_dir, _archive_cache, _source_root, **kwargs):
            self.assertIsInstance(kwargs["host_lock_fd"], int)
            for name in ("direct", "descendant"):
                if qualification.executor._pid_live(process_ids[name]):
                    raise qualification.executor.Refused(f"owned {name} is still live")
            journal_path = run_dir / "execution-status.json"
            journal = json.loads(journal_path.read_text())
            journal["phase"], journal["active"] = "interrupted", None
            journal_path.write_text(json.dumps(journal))
            return journal

        def execute(run_dir, _repository, archive_cache, source_root, **kwargs):
            def started(pid: int, pgid: int) -> None:
                process_ids.update(direct=pid, pgid=pgid)
                (run_dir / "execution-status.json").write_text(json.dumps({
                    "phase": "running",
                    "active": {
                        "release": "13.59", "stage": "generate",
                        "child": {"pid": pid, "pgid": pgid},
                    },
                }))
                os.close(write_fd)

            interrupter.start()
            qualification.executor._run_record(
                [sys.executable, "-c", child_program, str(write_fd)],
                cwd=self.root, env=dict(os.environ), run=subprocess.run, started=started,
            )
            self.fail("real child command unexpectedly returned after supervisor interruption")

        try:
            with patch.object(qualification.executor, "recover", side_effect=recover):
                with self.assertRaises(KeyboardInterrupt):
                    self.invoke(execute)
            interrupter.join(timeout=5)
            self.assertFalse(interrupter.is_alive(), "readiness thread did not observe the real child")
            if ready_error:
                raise ready_error[0]
            journal = json.loads((self.row_output / "before" / "execution-status.json").read_text())
            self.assertEqual(journal["phase"], "interrupted")
            self.assertIsNone(journal["active"])
            for name in ("direct", "descendant"):
                with self.subTest(process=name):
                    self.assertFalse(qualification.executor._pid_live(process_ids[name]))
            with qualification.executor._HostLock(self.lease):
                pass
        finally:
            for descriptor in (write_fd, read_fd):
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            pgid = process_ids.get("pgid")
            if pgid is not None:
                try:
                    os.killpg(pgid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            direct = process_ids.get("direct")
            if direct is not None:
                try:
                    os.waitpid(direct, 0)
                except ChildProcessError:
                    pass

    def test_recovery_refusal_does_not_mask_original_interruption(self) -> None:
        def interrupted(run_dir, _repository, _archive_cache, _source_root, **_kwargs):
            (run_dir / "execution-status.json").write_text(json.dumps({
                "phase": "running", "active": {"release": "13.59", "stage": "generate"},
            }))
            raise KeyboardInterrupt("original interruption")
        with patch.object(qualification.executor, "recover",
                          side_effect=qualification.executor.Refused("live child")):
            with self.assertRaisesRegex(KeyboardInterrupt, "original interruption") as caught:
                self.invoke(interrupted)
        self.assertTrue(any("durable interruption recovery failed" in note
                            for note in getattr(caught.exception, "__notes__", [])))

    def test_incomplete_owned_cleanup_prevents_durable_recovery_publication(self) -> None:
        def interrupted(run_dir, _repository, _archive_cache, _source_root, **_kwargs):
            (run_dir / "execution-status.json").write_text(json.dumps({
                "phase": "running", "active": {"release": "13.59", "stage": "generate"},
            }))
            failure = KeyboardInterrupt("original interruption")
            setattr(failure, "_oxidex_owned_child_cleanup", "incomplete")
            raise failure

        with patch.object(qualification.executor, "recover") as recover:
            with self.assertRaisesRegex(KeyboardInterrupt, "original interruption") as caught:
                self.invoke(interrupted)
        recover.assert_not_called()
        self.assertEqual(getattr(caught.exception, "_oxidex_durable_recovery"), "incomplete")
        self.assertTrue(any("owned child cleanup is incomplete" in note
                            for note in getattr(caught.exception, "__notes__", [])))

    def test_cli_reports_unverified_recovery_truthfully_and_retains_exit_130(self) -> None:
        interrupted = KeyboardInterrupt("original interruption")
        interrupted.add_note("durable interruption recovery failed: active child is still live")
        arguments = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--target-root", "target", "--lease", "lease", "--run-id", "run",
            "--owner-receipt", "owner", "--heartbeat-receipt", "heartbeat",
            "--expiry-receipt", "expiry", "--release-receipt", "release",
            "--handoff-receipt", "handoff",
        ]
        stderr = io.StringIO()
        with patch.object(qualification, "run_qualification", side_effect=interrupted), \
             redirect_stderr(stderr):
            self.assertEqual(qualification.main(arguments), 130)
        rendered = stderr.getvalue()
        self.assertIn("durable recovery incomplete or unverified", rendered)
        self.assertIn("active child is still live", rendered)
        self.assertNotIn("interrupted after durable recovery", rendered)

    def test_cli_claims_durable_recovery_only_when_wrapper_established_it(self) -> None:
        interrupted = KeyboardInterrupt("original interruption")
        setattr(interrupted, "_oxidex_durable_recovery", "recovered")
        arguments = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--target-root", "target", "--lease", "lease", "--run-id", "run",
            "--owner-receipt", "owner", "--heartbeat-receipt", "heartbeat",
            "--expiry-receipt", "expiry", "--release-receipt", "release",
            "--handoff-receipt", "handoff",
        ]
        stderr = io.StringIO()
        with patch.object(qualification, "run_qualification", side_effect=interrupted), \
             redirect_stderr(stderr):
            self.assertEqual(qualification.main(arguments), 130)
        self.assertEqual(
            stderr.getvalue(),
            "version transition qualification interrupted after durable recovery\n",
        )

    def test_cli_reports_recovered_with_secondary_cleanup_warning(self) -> None:
        interrupted = KeyboardInterrupt("original interruption")
        setattr(interrupted, "_oxidex_durable_recovery", "recovered")
        interrupted.add_note("bounded owned-child cleanup failed: diagnostic fault")
        arguments = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--target-root", "target", "--lease", "lease", "--run-id", "run",
            "--owner-receipt", "owner", "--heartbeat-receipt", "heartbeat",
            "--expiry-receipt", "expiry", "--release-receipt", "release",
            "--handoff-receipt", "handoff",
        ]
        stderr = io.StringIO()
        with patch.object(qualification, "run_qualification", side_effect=interrupted), \
             redirect_stderr(stderr):
            self.assertEqual(qualification.main(arguments), 130)
        rendered = stderr.getvalue()
        self.assertIn("interrupted after durable recovery", rendered)
        self.assertIn("recovery detail: bounded owned-child cleanup failed", rendered)
        self.assertNotIn("incomplete or unverified", rendered)

    def test_native_cases_are_frozen_before_first_side_and_rechecked(self) -> None:
        calls = 0
        def mutates_after_before(_run_dir, _repository, _archive_cache, _source_root, **_kwargs):
            nonlocal calls
            calls += 1
            if calls == 1:
                (self.root / "cases.json").write_text('[{"name":"narrower"}]')
            return {"phase": "complete", "scope": {"write_acceptance": "passed_per_release"}}
        with self.assertRaisesRegex(qualification.Refused, "native-case input changed"):
            self.invoke(mutates_after_before)
        self.assertEqual(calls, 1)

    def test_native_only_fixture_bytes_are_frozen_before_first_side_and_rechecked(self) -> None:
        calls = 0
        def mutates_after_before(_run_dir, _repository, _archive_cache, _source_root, **_kwargs):
            nonlocal calls
            calls += 1
            if calls == 1:
                self.native_fixture.write_bytes(b"replacement fixture")
            return {"phase": "complete", "scope": {"write_acceptance": "passed_per_release"}}
        with self.assertRaisesRegex(qualification.Refused, "native fixture input changed"):
            self.invoke(mutates_after_before)
        self.assertEqual(calls, 1)

    def test_native_fixture_binding_refuses_absent_or_symlink_input(self) -> None:
        cases = json.loads((self.root / "cases.json").read_text())
        self.native_fixture.unlink()
        with self.assertRaisesRegex(qualification.Refused, "existing regular file"):
            qualification._native_fixture_bindings(cases)
        target = self.root / "native-target.dat"
        target.write_bytes(b"native fixture")
        self.native_fixture.symlink_to(target)
        with self.assertRaisesRegex(qualification.Refused, "existing regular file"):
            qualification._native_fixture_bindings(cases)

    def test_fixture_manifest_is_frozen_before_first_side_and_rechecked(self) -> None:
        calls = 0
        def mutates_after_before(_run_dir, _repository, _archive_cache, _source_root, **_kwargs):
            nonlocal calls
            calls += 1
            if calls == 1:
                (self.root / "read.json").write_text("{}")
            return {"phase": "complete", "scope": {"write_acceptance": "passed_per_release"}}
        with self.assertRaisesRegex(qualification.Refused, "selected input changed"):
            self.invoke(mutates_after_before)
        self.assertEqual(calls, 1)

    def test_verified_source_identity_is_frozen_before_first_side_and_rechecked(self) -> None:
        calls = 0
        def mutates_after_before(_run_dir, _repository, _archive_cache, _source_root, **_kwargs):
            nonlocal calls
            calls += 1
            if calls == 1:
                self.identity["source_tree_sha256"] = "0" * 64
            return {"phase": "complete", "scope": {"write_acceptance": "passed_per_release"}}
        with self.assertRaisesRegex(qualification.Refused, "source input changed"):
            self.invoke(mutates_after_before)
        self.assertEqual(calls, 1)

    def test_stale_target_refuses_before_executor(self) -> None:
        self.row_target.mkdir(parents=True)
        execute = unittest.mock.Mock()
        with self.assertRaisesRegex(qualification.Refused, "stale reuse"):
            self.invoke(execute)
        execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()
