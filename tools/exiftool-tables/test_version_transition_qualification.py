#!/usr/bin/env python3
"""Contracts for the non-promoting Task19 transition wrapper."""

from __future__ import annotations

from contextlib import redirect_stderr
import io
import json
import io
import os
import signal
import subprocess
import stat
import sys
import threading
import time
from contextlib import redirect_stdout
from functools import partial
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from pathlib import Path


HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import version_transition_qualification as qualification
import test_version_rehearsal_native_oracle as native_fixture

LIVE_CHILD_EXIT = 5
CONTENDER = (
    "import fcntl, sys\n"
    "with open(sys.argv[1], 'r+') as stream:\n"
    "    try:\n"
    "        fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)\n"
    "    except BlockingIOError:\n"
    "        print('blocked')\n"
    "    else:\n"
    "        print('acquired')\n"
)


def _contend(lease: Path) -> str:
    """Ask an independent process whether it can take the transition lease now."""
    probe = subprocess.run([sys.executable, "-c", CONTENDER, str(lease)],
                           capture_output=True, text=True, timeout=20)
    if probe.returncode != 0:
        raise AssertionError(f"contender probe failed: {probe.stderr}")
    return probe.stdout.strip()


def _pid_live(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def _kill_own_session_child(pid: int) -> None:
    """Kill only the session-leader child this test's wrapper spawned, then await its exit."""
    try:
        if os.getpgid(pid) == pid:
            os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 10
    while _pid_live(pid) and time.monotonic() < deadline:
        time.sleep(0.02)


def _interrupted_live_child_wrapper(root_text: str) -> int:
    """Subprocess entry: the real CLI, lease, recovery and executor spawn.

    Only preflight inputs are substituted. The body spawns a real stage child
    through the executor's production ``Popen(close_fds=False)`` branch while
    borrowing the lease, then waits on it until the test interrupts this
    process with SIGINT. Interrupt cleanup reaps a killable child, so both
    cleanup passes are made to fail without signalling it: the child survives
    exactly as one whose exit cannot be proven would, which is the case the
    fail-closed lease must hold for.
    """
    root = Path(root_text)
    temporary, capture, catalog, plan, resolution, materialization, cache, sources, release = (
        native_fixture.make_state()
    )
    try:
        output, target = root / "output", root / "target"
        output.mkdir()
        target.mkdir()
        lease = output / "transition.host.lock"
        lease.touch()
        run_id = "live-child"
        receipt_root = output / run_id
        fixture = root / "fixture.jpg"
        fixture.write_bytes(b"\xff\xd8fixture")
        binding = {"path": str(fixture), "sha256": qualification._sha_file(fixture),
                   "bytes": fixture.stat().st_size}
        (root / "read.json").write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest", "fixtures": [binding],
        }))
        (root / "write.json").write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_write_fixture_manifest", "fixtures": [binding],
        }))
        (root / "cases.json").write_text(json.dumps([{
            "name": "case", "fixture": str(fixture),
            "read": {"query": "Comment", "expectation": "native_unsupported"},
            "write": {"tag": "Comment", "operation": "delete", "readback": None},
        }]))
        row_id = f"same-pin-{release}"
        row = {
            "id": row_id, "before_version": release, "after_version": release,
            "immutable_source_identities": {
                side: {"expected_release": release, "input_bundle": str(root / "bundle")}
                for side in qualification.SIDES
            },
            "fixtures": {
                side: {"read_manifest": str(root / "read.json"), "write_manifest": str(root / "write.json"),
                       "native_cases": str(root / "cases.json")}
                for side in qualification.SIDES
            },
            "artifact_manifest": {"comparison": "identical"},
            "target_directory": str(target / run_id / row_id),
            "durable_output_directory": str(output / run_id / row_id),
        }
        identity = {
            "release": release, "tag_object": "b" * 40, "peeled_commit": "c" * 40,
            "source_directory": "source", "source_tree_sha256": "d" * 64,
            "materialization_sha256": "e" * 64, "bundle": str(root / "bundle"),
            "archive_cache": str(cache), "source_root": str(sources),
            "documents": {"capture": capture, "catalog": catalog, "plan": plan,
                          "resolution": resolution, "materialization": materialization},
        }
        caller = {"pin_version": release, "head": plan["repository_commit"]}

        def execute(run_dir, _repository, _archive_cache, _source_root, *, host_lock_fd, stage_guard):
            journal = json.loads((run_dir / "execution-status.json").read_text())
            journal["phase"] = "running"
            journal["active"] = {"release": release, "stage": "generate"}
            journal["releases"][release]["stages"]["generate"] = "running"
            qualification.executor._store_journal(run_dir, journal)

            def started(pid, pgid):
                journal["active"]["child"] = {"pid": pid, "pgid": pgid}
                qualification.executor._store_journal(run_dir, journal)
                (root / "child.pid").write_text(str(pid))

            with host_lock_fd.borrow(lease):
                stage_guard(release, "generate", "before")
                qualification.executor._run_record(
                    [sys.executable, "-c", "import time; time.sleep(60)"], cwd=root,
                    env=dict(os.environ), run=subprocess.run, started=started,
                )
            raise AssertionError("the wrapper must be interrupted while its stage child is live")

        arguments = [
            "--matrix", str(qualification.CANONICAL_MATRIX), "--repository", str(qualification.REPOSITORY_ROOT),
            "--output", str(output),
            "--target-root", str(target), "--lease", str(lease), "--run-id", run_id,
            "--owner-receipt", str(receipt_root / "lease-owner.json"),
            "--heartbeat-receipt", str(receipt_root / "lease-heartbeat.jsonl"),
            "--expiry-receipt", str(receipt_root / "lease-expiry.json"),
            "--release-receipt", str(receipt_root / "lease-release.json"),
            "--handoff-receipt", str(receipt_root / "handoff.jsonl"),
        ]
        with patch.object(qualification, "snapshot_caller", return_value=caller), \
             patch.object(qualification, "verify_caller"), \
             patch.object(qualification, "load_matrix", return_value={"rows": [row]}), \
             patch.object(qualification, "materialize_matrix", return_value={"rows": [row]}), \
             patch.object(qualification, "_perl", return_value=Path(sys.executable).resolve()), \
             patch.object(qualification, "resolve_source_identity", return_value=identity), \
             patch.object(qualification, "run_qualification",
                          partial(qualification.run_qualification, execute=execute)), \
             patch.object(qualification.executor, "_bounded_timeout_cleanup",
                          side_effect=OSError("cleanup cannot prove the child gone")), \
             patch.object(qualification.executor, "_emergency_reap_group",
                          side_effect=OSError("emergency cleanup cannot prove the child gone")):
            return qualification.main(arguments)
    finally:
        temporary.cleanup()


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
        # Temporary fixture roots are outside the ops root: the fence refuses them.
        with self.assertRaisesRegex(qualification.Refused, "verified archive cache"):
            qualification.resolve_source_identity(
                {"expected_release": release, "expected_peeled_commit": side["peeled_commit"]}, bundle,
            )
        fence = patch.object(qualification, "_evidence_location", side_effect=lambda value, _label: Path(value))
        fence.start(); self.addCleanup(fence.stop)
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
    def test_side_receipt_preserves_read_instrument_identity(self) -> None:
        with TemporaryDirectory() as temporary:
            run_dir = Path(temporary)
            (run_dir / "execution-status.json").write_text("{}")
            source_commit = "a" * 40
            read = {
                "source_commit": source_commit,
                "classification_counts": {"matched": 1, "value_diff": 0, "missing": 0,
                                          "renames": 0, "extra": 0},
                "binary": {"path": "/isolated/oxidex", "sha256": "b" * 64, "bytes": 123},
                "native_identity": {"release": "13.59", "perl": {"path": "/pinned/perl", "sha256": "c" * 64},
                                    "source": {"path": "/pinned/exiftool"},
                                    "lib": {"path": "/pinned/lib", "exiftool_pm_sha256": "d" * 64}},
                "native_probe_sha256": "e" * 64,
                "fixtures": {"manifest": "/pinned/read-fixtures.json", "manifest_sha256": "f" * 64,
                             "entries": [{"source": "/pinned/sample.jpg"}]},
            }
            native = {"state": "ready", "probe_sha256": "e" * 64,
                      "version": {"state": "ok", "stdout": "13.59\n"},
                      "perl_capability": {"available": True},
                      "docx_capability": {"state": "ok", "stdout": "DOCX\n"}}
            log = run_dir / "test-command.json"
            log.write_text('{"commands": []}')
            log_binding = {"path": str(log), "sha256": qualification._sha_file(log)}
            suite_commands = [
                {"argv": list(argv), "exit": 0, "duration_seconds": 3.0, "passed": 8, "failed": 0,
                 "ignored": 2, "measured": 0, "filtered_out": 0, "targets": 4}
                for argv in qualification.stage_adapter.TEST_COMMANDS
            ]
            oracle_cache = "/isolated/target/test-suite/exiftool-oracle"
            exiftool_oracle = {
                "cache_dir": oracle_cache, "tree": oracle_cache + "/exiftool",
                "tree_realpath": "/pinned/exiftool",
                "program": {"path": oracle_cache + "/exiftool/exiftool", "sha256": "1" * 64},
                "lib": {"path": oracle_cache + "/exiftool/lib", "exiftool_pm_sha256": "d" * 64},
                "perl": {"path": "/pinned/perl", "sha256": "c" * 64},
                "version": "13.59", "docx_filetype": "DOCX", "perl_modules_available": True,
                "perl_modules": {"Archive::Zip": True},
                "path_shim": {"path": oracle_cache + "/bin/exiftool", "sha256": "2" * 64},
            }
            environment = {
                "PATH": oracle_cache + "/bin" + os.pathsep + "/usr/bin", "HOME": "/Users/test",
                "CARGO_TARGET_DIR": "/isolated/target/test-suite", "CARGO_TERM_COLOR": "never",
                "EXIFTOOL_CACHE_DIR": oracle_cache, "EXIFTOOL_PERL": "/pinned/perl",
                "OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES": "1",
            }
            fixture_corpus = {
                "ops_root": "/durable/ops", "bootstrap_pin": "13.59", "version_independent": True,
                "corpus": "/durable/ops/cache/exiftool/13.59/combined-samples",
                "link": oracle_cache + "/combined-samples", "corpus_tree_sha256": "7" * 64,
                "manifest": {"path": "/durable/ops/cache/exiftool/13.59/combined-samples.manifest",
                             "sha256": "8" * 64, "file_count": 4249},
                "storage_manifest": {"path": "/durable/ops/evidence/storage-manifest.json", "sha256": "9" * 64},
                "verify_command": ["python3", "/checkout/tools/release/bootstrap_oracle.py", "verify",
                                   "--root", "/durable/ops", "--pin", "13.59"],
                "verified_before_run": True, "verified_after_run": True,
            }
            authority = {"ops_root": "/durable/ops", "bootstrap_pin": "13.59",
                         "corpus": fixture_corpus["corpus"], "corpus_tree_sha256": "7" * 64,
                         "manifest": dict(fixture_corpus["manifest"])}
            corpus_authority = patch.object(qualification, "_fixture_corpus_authority", return_value=authority)
            corpus_authority.start(); self.addCleanup(corpus_authority.stop)
            release_tests = {
                "state": "passed", "denominator": 8, "raw_report": log_binding,
                "native_identity": read["native_identity"],
                "test_suite": {"commands": suite_commands, "log": log_binding,
                               "target_directory": "/isolated/target/test-suite",
                               "exiftool_oracle": exiftool_oracle, "environment": environment,
                               "fixture_corpus": fixture_corpus,
                               "cargo_config": {"checked": ["/durable/.cargo/config.toml"], "outside_checkout": []},
                               "totals": {"passed": 8, "failed": 0, "ignored": 2, "measured": 0,
                                          "filtered_out": 0, "targets": 4}},
            }
            pin_commit, brew_commit = "8bab26f4f68e0e26f0bb7960be334d5b520ea452", "48a229ceaefd4985c50990b14116b6d856af0985"
            checkout = run_dir / "checkouts" / qualification.executor._safe_name("13.59")
            checkout.mkdir(parents=True)
            (checkout / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.97.1"\n')
            build_environment = {
                "environment": {"PATH": "/usr/bin", "HOME": "/Users/test", "CARGO_TARGET_DIR": "/isolated/target",
                                "CARGO_TERM_COLOR": "never"},
                "toolchain": {"rustc": f"rustc 1.97.1 (x 2026-01-01)\nbinary: rustc\ncommit-hash: {pin_commit}\n"
                                       "host: aarch64-apple-darwin\nrelease: 1.97.1",
                              "cargo": "cargo 1.97.1 (x 2026-01-01)"},
                "cargo_config": {"checked": ["/Users/test/.cargo/config.toml"], "outside_checkout": []},
                "toolchain_pin": {"file": "rust-toolchain.toml", "channel": "1.97.1",
                                  "sha256": qualification._sha_file(checkout / "rust-toolchain.toml")},
                "compiled_by": {"binary": [pin_commit], "writer_binary": [pin_commit]},
                "rustc_path": "/Users/test/.cargo/bin/rustc",
                "pin_rustc": {"release": "1.97.1", "commit_hash": pin_commit,
                              "path": "/Users/test/.rustup/toolchains/1.97.1/bin/rustc"},
            }
            compiler = {"toolchain": dict(build_environment["toolchain"]),
                        "toolchain_pin": dict(build_environment["toolchain_pin"]),
                        "rustc_path": "/Users/test/.cargo/bin/rustc",
                        "pin_rustc": dict(build_environment["pin_rustc"])}
            release_tests["test_suite"]["compiler"] = compiler
            build = {"binary": {"path": "/isolated/target/debug/oxidex", "sha256": "b" * 64, "bytes": 123},
                     "build_environment": build_environment}
            reports = {"generate": {"generated_artifacts": []}, "read": read, "write": {}, "native": native,
                       "test": release_tests, "build": build}
            identity = {name: "identity" for name in (
                "release", "tag_object", "peeled_commit", "source_directory",
                "source_tree_sha256", "materialization_sha256",
            )}
            journal = {"phase": "complete", "scope": {"write_acceptance": "passed_per_release",
                                                      "release_tests": "passed_per_release"}}
            with patch.object(qualification, "_report_for", side_effect=lambda _dir, _journal, _release, stage: reports[stage]), \
                 patch.object(qualification.stage_adapter, "generated_refusal_counts", return_value={"total": 0, "counters": []}):
                side = qualification._side_receipt(run_dir, journal, "13.59", identity)
            self.assertEqual(side.get("release_tests"), {
                "commands": [list(argv) for argv in qualification.stage_adapter.TEST_COMMANDS],
                "exits": [0], "passed": 8, "failed": 0, "ignored": 2, "measured": 0,
                "filtered_out": 0, "targets": 4, "duration_seconds": 3.0,
                "target_directory": "/isolated/target/test-suite", "log": log_binding,
                "exiftool_oracle": exiftool_oracle, "fixture_corpus": fixture_corpus,
                "compiler": compiler,
            })
            self.assertEqual(side.get("build_environment"), build_environment)

            def build_refused(label, mutate):
                broken = json.loads(json.dumps(build))
                mutate(broken)
                reports["build"] = broken
                with self.subTest(label=label), \
                     patch.object(qualification, "_report_for",
                                  side_effect=lambda _dir, _journal, _release, stage: reports[stage]), \
                     patch.object(qualification.stage_adapter, "generated_refusal_counts",
                                  return_value={"total": 0, "counters": []}):
                    with self.assertRaisesRegex(qualification.Refused, "build environment"):
                        qualification._side_receipt(run_dir, journal, "13.59", identity)
                reports["build"] = build

            for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER",
                        "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_TARGET"):
                build_refused(f"ambient {key}", lambda r, key=key: r["build_environment"]["environment"].update({key: "x"}))
            build_refused("environment absent", lambda r: r.pop("build_environment"))
            build_refused("toolchain absent", lambda r: r["build_environment"].pop("toolchain"))
            build_refused("rustc unidentified", lambda r: r["build_environment"]["toolchain"].update(rustc=""))
            build_refused("cargo config found", lambda r: r["build_environment"]["cargo_config"].update(
                outside_checkout=["/Users/test/.cargo/config.toml"]))
            build_refused("binary outside the build target", lambda r: r["binary"].update(path="/elsewhere/oxidex"))
            build_refused("resolved rustc absent", lambda r: r["build_environment"].pop("rustc_path"))
            build_refused("rustup pin identity absent", lambda r: r["build_environment"].pop("pin_rustc"))
            build_refused("non-rustup rustc reporting the pinned release", lambda r: (
                r["build_environment"]["toolchain"].update(
                    rustc=f"rustc 1.97.1 (d 2026-01-01)\ncommit-hash: {'2' * 40}\nrelease: 1.97.1"),
                r["build_environment"]["compiled_by"].update(binary=["2" * 40], writer_binary=["2" * 40])))
            build_refused("rustup pin of another release", lambda r: r["build_environment"]["pin_rustc"].update(
                release="1.98.1"))
            build_refused("resolved rustc relative", lambda r: r["build_environment"].update(rustc_path="rustc"))
            build_refused("toolchain pin absent", lambda r: r["build_environment"].pop("toolchain_pin"))
            build_refused("compiler fingerprint absent", lambda r: r["build_environment"].pop("compiled_by"))
            build_refused("PATH resolved Homebrew rustc", lambda r: r["build_environment"]["toolchain"].update(
                rustc=f"rustc 1.98.1 (h 2026-09-01) (Homebrew)\ncommit-hash: {brew_commit}\nrelease: 1.98.1"))
            build_refused("rustc without a release line", lambda r: r["build_environment"]["toolchain"].update(
                rustc="rustc 1.97.1 (x 2026-01-01)\nhost: aarch64-apple-darwin"))
            build_refused("cargo off the pin", lambda r: r["build_environment"]["toolchain"].update(
                cargo="cargo 1.98.1 (h 2026-08-05) (Homebrew)"))
            build_refused("CLI compiled by another rustc", lambda r: r["build_environment"]["compiled_by"].update(
                binary=[brew_commit]))
            build_refused("writer compiled by two rustcs", lambda r: r["build_environment"]["compiled_by"].update(
                writer_binary=[brew_commit, pin_commit]))
            build_refused("recorded pin differs from the checkout's", lambda r: r["build_environment"][
                "toolchain_pin"].update(channel="1.98.1"))
            (checkout / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.98.1"\n')
            build_refused("checkout's pin moved since the build", lambda r: None)
            (checkout / "rust-toolchain.toml").unlink()
            build_refused("checkout has no pin", lambda r: None)
            (checkout / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.97.1"\n')

            def refused(label, mutate, pattern="release test suite"):
                broken = json.loads(json.dumps(release_tests))
                mutate(broken)
                reports["test"] = broken
                with self.subTest(label=label), \
                     patch.object(qualification, "_report_for",
                                  side_effect=lambda _dir, _journal, _release, stage: reports[stage]), \
                     patch.object(qualification.stage_adapter, "generated_refusal_counts",
                                  return_value={"total": 0, "counters": []}):
                    with self.assertRaisesRegex(qualification.Refused, pattern):
                        qualification._side_receipt(run_dir, journal, "13.59", identity)

            refused("failed tests", lambda r: r["test_suite"]["totals"].update(failed=1))
            refused("suite compiler absent", lambda r: r["test_suite"].pop("compiler"), "pinned toolchain")
            refused("suite rustup pin absent", lambda r: r["test_suite"]["compiler"].pop("pin_rustc"),
                    "pinned toolchain")
            refused("suite rustc is not rustup's pin", lambda r: r["test_suite"]["compiler"]["pin_rustc"].update(
                commit_hash="3" * 40), "pinned toolchain")
            refused("suite ran on Homebrew rustc", lambda r: r["test_suite"]["compiler"]["toolchain"].update(
                rustc=f"rustc 1.98.1 (h 2026-09-01) (Homebrew)\ncommit-hash: {brew_commit}\nrelease: 1.98.1"),
                "pinned toolchain")
            refused("suite cargo off the pin", lambda r: r["test_suite"]["compiler"]["toolchain"].update(
                cargo="cargo 1.98.1 (h 2026-08-05) (Homebrew)"), "pinned toolchain")
            refused("suite pin is not the checkout's", lambda r: r["test_suite"]["compiler"]["toolchain_pin"].update(
                channel="1.98.1"), "pinned toolchain")
            refused("suite rustc unresolved", lambda r: r["test_suite"]["compiler"].update(rustc_path=None),
                    "pinned toolchain")
            refused("suite rustc is not the build's", lambda r: r["test_suite"]["compiler"]["toolchain"].update(
                rustc=f"rustc 1.97.1 (y 2026-01-01)\ncommit-hash: {'1' * 40}\nrelease: 1.97.1"), "pinned toolchain")
            refused("failed command", lambda r: r["test_suite"]["commands"][0].update(exit=101))
            refused("homebrew oracle", lambda r: r["test_suite"]["exiftool_oracle"].update(version="13.55"))
            refused("degraded oracle", lambda r: r["test_suite"]["exiftool_oracle"].update(docx_filetype="ZIP"))
            refused("module missing", lambda r: r["test_suite"]["exiftool_oracle"].update(perl_modules_available=False))
            refused("foreign tree", lambda r: r["test_suite"]["exiftool_oracle"].update(tree_realpath="/opt/homebrew/exiftool"))
            refused("foreign lib", lambda r: r["test_suite"]["exiftool_oracle"]["lib"].update(exiftool_pm_sha256="0" * 64))
            refused("foreign perl", lambda r: r["test_suite"]["exiftool_oracle"]["perl"].update(sha256="0" * 64))
            refused("oracle absent", lambda r: r["test_suite"].pop("exiftool_oracle"))
            refused("ambient EXIFTOOL", lambda r: r["test_suite"]["environment"].update(EXIFTOOL="/opt/homebrew/bin/exiftool"))
            refused("skew allowed", lambda r: r["test_suite"]["environment"].update(OXIDEX_ALLOW_EXIFTOOL_SKEW="1"))
            refused("foreign cache", lambda r: r["test_suite"]["environment"].update(EXIFTOOL_CACHE_DIR="/tmp/foreign"))
            refused("fixtures optional", lambda r: r["test_suite"]["environment"].pop("OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES"))
            refused("path shim bypassed", lambda r: r["test_suite"]["environment"].update(PATH="/opt/homebrew/bin"))
            refused("ambient cargo config", lambda r: r["test_suite"]["cargo_config"].update(
                outside_checkout=["/Users/test/.cargo/config.toml"]))
            refused("cargo config unchecked", lambda r: r["test_suite"].pop("cargo_config"))
            refused("corpus absent", lambda r: r["test_suite"].pop("fixture_corpus"))
            refused("corpus manifest differs", lambda r: r["test_suite"]["fixture_corpus"]["manifest"].update(sha256="0" * 64))
            refused("corpus count differs", lambda r: r["test_suite"]["fixture_corpus"]["manifest"].update(file_count=12))
            refused("corpus tree differs", lambda r: r["test_suite"]["fixture_corpus"].update(corpus_tree_sha256="0" * 64))
            refused("foreign corpus", lambda r: r["test_suite"]["fixture_corpus"].update(corpus="/tmp/samples"))
            refused("foreign ops root", lambda r: r["test_suite"]["fixture_corpus"].update(ops_root="/tmp/ops"))
            refused("corpus not linked", lambda r: r["test_suite"]["fixture_corpus"].update(link="/tmp/elsewhere"))
            refused("not verified before", lambda r: r["test_suite"]["fixture_corpus"].update(verified_before_run=False))
            refused("not verified after", lambda r: r["test_suite"]["fixture_corpus"].update(verified_after_run=False))
            refused("failed state", lambda r: r.update(state="failed"))
            refused("no tests", lambda r: r["test_suite"]["totals"].update(passed=0))
            refused("totals disagree", lambda r: r["test_suite"]["totals"].update(ignored=3))
            refused("weaker command", lambda r: r["test_suite"]["commands"][0].update(argv=["cargo", "test", "--lib"]))
            refused("split doc command", lambda r: r["test_suite"]["commands"].append(
                dict(r["test_suite"]["commands"][0], argv=["cargo", "test", "--doc"])))
            refused("string count", lambda r: r["test_suite"]["totals"].update(passed="8"))
            refused("log differs", lambda r: r["test_suite"].update(log={"path": str(log), "sha256": "0" * 64}))
            refused("malformed", lambda r: r.pop("test_suite"))
            reports["test"] = release_tests
            log.write_text("changed")
            with patch.object(qualification, "_report_for", side_effect=lambda _dir, _journal, _release, stage: reports[stage]), \
                 patch.object(qualification.stage_adapter, "generated_refusal_counts", return_value={"total": 0, "counters": []}):
                with self.assertRaisesRegex(qualification.Refused, "release test suite"):
                    qualification._side_receipt(run_dir, journal, "13.59", identity)
            log.write_text('{"commands": []}')
            for scope in ({"write_acceptance": "passed_per_release"},
                          {"write_acceptance": "passed_per_release",
                           "release_tests": "unsupported_for_one_or_more_releases"}):
                with self.subTest(scope=scope), \
                     patch.object(qualification, "_report_for", side_effect=lambda _dir, _journal, _release, stage: reports[stage]), \
                     patch.object(qualification.stage_adapter, "generated_refusal_counts", return_value={"total": 0, "counters": []}):
                    with self.assertRaisesRegex(qualification.Refused, "release test suite"):
                        qualification._side_receipt(run_dir, {"phase": "complete", "scope": scope}, "13.59", identity)
            self.assertEqual(side.get("instrument"), {
                "source_commit": source_commit,
                "binary": read["binary"],
                "native_identity": read["native_identity"],
                "native_probe_sha256": read["native_probe_sha256"],
                "read_fixture_manifest": "/pinned/read-fixtures.json",
                "read_fixture_manifest_sha256": "f" * 64,
                "read_fixture_count": 1,
                "capability_probe": {"state": "ready", "version": "13.59", "docx_filetype": "DOCX",
                                     "perl_modules_available": True},
            })
            for broken in ({"docx_capability": {"state": "ok", "stdout": "ZIP\n"}},
                           {"probe_sha256": "0" * 64}, {"state": "failed"},
                           {"version": {"state": "ok", "stdout": "13.55\n"}}):
                reports["native"] = {**native, **broken}
                with self.subTest(broken=broken), \
                     patch.object(qualification, "_report_for",
                                  side_effect=lambda _dir, _journal, _release, stage: reports[stage]), \
                     patch.object(qualification.stage_adapter, "generated_refusal_counts",
                                  return_value={"total": 0, "counters": []}):
                    with self.assertRaisesRegex(qualification.Refused, "capability probe"):
                        qualification._side_receipt(run_dir, journal, "13.59", identity)

    def test_fixture_corpus_authority_reads_the_bootstrap_verified_manifest(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = json.loads((qualification.REPOSITORY_ROOT / "tools/release/oracle-lock.json").read_text())
            corpus = root / "cache/exiftool/13.59/combined-samples"
            corpus.mkdir(parents=True)
            (corpus / "a.jpg").write_bytes(b"a")
            manifest = corpus.parent / "combined-samples.manifest"
            manifest.write_text(f"{qualification._sha_file(corpus / 'a.jpg')}  a.jpg\n")
            storage = root / "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap/storage-manifest.json"
            storage.parent.mkdir(parents=True)

            def bind(manifest_sha, tree_sha):
                storage.write_text(json.dumps({"artifacts": {
                    "corpus_manifest": {"kind": "file", "path": str(manifest), "sha256": manifest_sha},
                    "corpus_tree": {"kind": "tree", "path": str(corpus), "sha256": tree_sha}}}))

            with patch.object(qualification.ops_paths, "ops_root", return_value=root):
                bind(qualification._sha_file(manifest), lock["corpus_tree_sha256"])
                self.assertEqual(qualification._fixture_corpus_authority(), {
                    "ops_root": str(root), "bootstrap_pin": "13.59", "corpus": str(corpus),
                    "corpus_tree_sha256": lock["corpus_tree_sha256"],
                    "manifest": {"path": str(manifest), "sha256": qualification._sha_file(manifest),
                                 "file_count": 1},
                })
                for manifest_sha, tree_sha in (("0" * 64, lock["corpus_tree_sha256"]),
                                               (qualification._sha_file(manifest), "0" * 64)):
                    bind(manifest_sha, tree_sha)
                    with self.assertRaisesRegex(qualification.Refused, "not bootstrap-verified"):
                        qualification._fixture_corpus_authority()

    def test_output_and_input_evidence_roots_are_fenced_under_the_ops_root(self) -> None:
        matrix = qualification.load_matrix(qualification.CANONICAL_MATRIX, "13.59")
        ops = Path("/durable/ops")
        with patch.object(qualification.ops_paths, "ops_root", return_value=ops):
            materialized = qualification.materialize_matrix(
                matrix, output_root=ops / "evidence/task19", target_root=Path("/durable/targets"), run_id="run")
            self.assertTrue(all(row["durable_output_directory"].startswith("/durable/ops/evidence/task19/")
                                for row in materialized["rows"]))
            with self.assertRaisesRegex(qualification.Refused, "beneath the ops root"):
                qualification.materialize_matrix(matrix, output_root=Path("/durable/elsewhere"),
                                                 target_root=Path("/durable/targets"), run_id="run")
            with self.assertRaisesRegex(qualification.Refused, "durable"):
                qualification.materialize_matrix(matrix, output_root=Path("/tmp/task19"),
                                                 target_root=Path("/durable/targets"), run_id="run")
            self.assertEqual(qualification._evidence_location("/durable/ops/cache/archives", "archive cache"),
                             ops / "cache/archives")
            for value in ("/tmp/cache", "/durable/elsewhere/cache", "relative/cache"):
                with self.subTest(value=value):
                    with self.assertRaisesRegex(qualification.Refused, "archive cache"):
                        qualification._evidence_location(value, "archive cache")

    def test_documented_commands_default_the_ops_root_like_ops_paths(self) -> None:
        import re
        documents = (qualification.REPOSITORY_ROOT / "docs/UPGRADE-NEXT-STEPS.md",
                     qualification.REPOSITORY_ROOT
                     / "docs/superpowers/plans/2026-09-19-generated-runtime-release-functional-completion.md")
        for document in documents:
            text = document.read_text(encoding="utf-8")
            blocks = [block for block in re.findall(r"```bash\n(.*?)```", text, re.S)
                      if re.search(r"python3 \S*tools/exiftool-tables/version_transition_qualification\.py", block)]
            with self.subTest(document=document.name):
                self.assertTrue(blocks)
                for block in blocks:
                    self.assertNotRegex(block, r"\$OXIDEX_OPS_DIR\b|\$\{OXIDEX_OPS_DIR\}",
                                        "an unset OXIDEX_OPS_DIR must fall back to $HOME/oxidex-ops")
                    self.assertIn("${OXIDEX_OPS_DIR:-$HOME/oxidex-ops}", block)

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
            argv = normalized["commands"]["test"]["argv"]
            self.assertEqual(argv[1:3], ["{checkout}/tools/exiftool-tables/version_rehearsal_stage_adapter.py",
                                         "test"])

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
            # Interrupted between stages: still running, nothing active.
            (run_dir / "execution-status.json").write_text(json.dumps({"phase": "running", "active": None}))
            with patch.object(qualification.executor, "recover") as recover:
                qualification._recover_if_running(run_dir, Path("cache"), Path("sources"), host_lock_fd=17)
            recover.assert_called_once_with(run_dir, Path("cache"), Path("sources"), host_lock_fd=17)

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
            capability = getattr(held, "host_lock_capability", None)
            self.assertIsNotNone(capability, "transition lease must lend its owned lock")
            qualification._recover_if_running(run_dir, cache, sources, host_lock_fd=capability)
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
    def test_expired_owner_still_lends_held_lock_for_interruption_recovery(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lease_path = root / "transition.host.lock"
            lease_path.touch()
            with qualification.TransitionLease(
                lease=lease_path, run_id="expired-recovery", owner_receipt=root / "owner.json",
                heartbeat_receipt=root / "heartbeat.jsonl", expiry_receipt=root / "expiry.json",
                release_receipt=root / "release.json",
            ) as lease:
                with patch.object(qualification.time, "time", return_value=lease.expires_at + 1):
                    capability = lease.host_lock_capability
                with capability.borrow(lease_path):
                    self.assertTrue(lease_path.is_file())

    def test_live_receipt_cadence_cannot_be_treated_as_shutdown(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lease_path = root / "transition.host.lock"
            lease_path.touch()
            with qualification.TransitionLease(
                lease=lease_path, run_id="cadence", owner_receipt=root / "owner.json",
                heartbeat_receipt=root / "heartbeat.jsonl", expiry_receipt=root / "expiry.json",
                release_receipt=root / "release.json",
            ) as lease:
                cadence = qualification.ReceiptCadence(lease, root / "handoff.jsonl")
                cadence.__enter__()
                with patch.object(cadence.thread, "join", return_value=None), \
                     patch.object(cadence.thread, "is_alive", return_value=True):
                    with self.assertRaisesRegex(qualification.Refused, "cadence.*running"):
                        cadence.__exit__(None, None, None)

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

    def test_no_liveness_record_can_follow_lease_release(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lease_path = root / "transition.host.lock"; lease_path.touch()
            heartbeat = root / "heartbeat.jsonl"
            lease = qualification.TransitionLease(
                lease=lease_path, run_id="late", owner_receipt=root / "owner.json",
                heartbeat_receipt=heartbeat, expiry_receipt=root / "expiry.json",
                release_receipt=root / "release.json",
            )
            with lease:
                lease.finish("body-validated")
            recorded = heartbeat.read_text()
            with self.assertRaisesRegex(qualification.Refused, "not held"):
                lease.heartbeat("late", None, None)
            self.assertEqual(heartbeat.read_text(), recorded)

    def test_lease_release_waits_for_an_in_flight_periodic_record(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lease_path = root / "transition.host.lock"; lease_path.touch()
            handoff, release = root / "handoff.jsonl", root / "release.json"
            lease = qualification.TransitionLease(
                lease=lease_path, run_id="cadence-order", owner_receipt=root / "owner.json",
                heartbeat_receipt=root / "heartbeat.jsonl", expiry_receipt=root / "expiry.json",
                release_receipt=release,
            )
            entered, proceed = threading.Event(), threading.Event()
            original = qualification._append_jsonl

            def blocking_handoff(target, value):
                if target == handoff and value.get("state") == "periodic" and not proceed.is_set():
                    entered.set()
                    proceed.wait(10)
                return original(target, value)

            with patch.object(qualification, "_append_jsonl", side_effect=blocking_handoff):
                lease.__enter__()
                cadence = qualification.ReceiptCadence(lease, handoff, interval_seconds=0.01)
                cadence.__enter__()
                try:
                    self.assertTrue(entered.wait(5))
                    lease.finish("body-validated")
                    exiting = threading.Thread(target=lease.__exit__, args=(None, None, None))
                    exiting.start()
                    time.sleep(0.3)
                    self.assertFalse(release.exists(), "lease released during an in-flight liveness record")
                finally:
                    proceed.set()
                    exiting.join(10)
                    cadence.stop.set()
                    cadence.thread.join(10)
            released_at = json.loads(release.read_text())["released_at"]
            self.assertTrue(all(json.loads(line)["timestamp"] <= released_at
                                for line in handoff.read_text().splitlines()))

    def test_atomic_and_new_jsonl_receipts_fsync_their_directory(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            events = []
            original_fsync, original_replace = os.fsync, os.replace

            def fsync(descriptor):
                events.append("fsync-dir" if stat.S_ISDIR(os.fstat(descriptor).st_mode) else "fsync-file")
                return original_fsync(descriptor)

            def replace(source, target):
                events.append("replace")
                return original_replace(source, target)

            with patch.object(qualification.os, "fsync", side_effect=fsync), \
                 patch.object(qualification.os, "replace", side_effect=replace):
                qualification._atomic_json(root / "new" / "receipt.json", {"run_id": "durable"})
                self.assertIn("fsync-dir", events[events.index("replace"):])
                events.clear()
                qualification._append_jsonl(root / "log" / "records.jsonl", {"run_id": "durable"})
                self.assertEqual(events[0], "fsync-file")
                self.assertIn("fsync-dir", events[1:])

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
        # Owned-child and retained-lock state is process-wide by design; a
        # child one test leaves unproven must not fail an unrelated release.
        for name, value in (("_OWNED", qualification.executor._OwnedChildren()),
                            ("_RETAINED_LOCKS", [])):
            patcher = patch.object(qualification.executor, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
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

    def invoke(self, execute, *, matrix_path=None, repository=None):
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
                matrix_path=matrix_path or qualification.CANONICAL_MATRIX,
                repository=repository or qualification.REPOSITORY_ROOT,
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
                            and type(call[-1]["host_lock_fd"]).__name__ == "_HeldHostLock"
                            for call in calls))
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

    def test_release_receipt_replace_failure_leaves_no_committed_result(self) -> None:
        original_replace = qualification.os.replace

        def refuse_release_replace(source, target):
            if target == self.receipts["release_receipt"]:
                raise OSError("simulated release replacement failure")
            return original_replace(source, target)

        with patch.object(qualification.os, "replace", side_effect=refuse_release_replace):
            with self.assertRaisesRegex(qualification.Refused, "release receipt"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertFalse((self.output / self.run_id / "qualification-result.json").exists())
        self.assertEqual(json.loads(self.receipts["expiry_receipt"].read_text())[
            "qualification_outcome"], "pending")
        self.assertFalse(self.receipts["release_receipt"].exists())
        with qualification.executor._HostLock(self.lease):
            pass

    def test_handoff_write_failure_leaves_no_committed_result(self) -> None:
        original_append = qualification._append_jsonl

        def refuse_handoff(target, value):
            if target == self.receipts["handoff_receipt"]:
                raise OSError("simulated handoff write failure")
            return original_append(target, value)

        with patch.object(qualification, "_append_jsonl", side_effect=refuse_handoff):
            with self.assertRaisesRegex(OSError, "handoff write failure"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertFalse((self.output / self.run_id / "qualification-result.json").exists())
        self.assertEqual(json.loads(self.receipts["release_receipt"].read_text())[
            "qualification_outcome"], "pending")

    def test_owner_receipt_open_failure_leaves_no_committed_result(self) -> None:
        original_open = Path.open

        def refuse_owner_open(path, *args, **kwargs):
            if path.name.startswith(".lease-owner.json.") and path.name.endswith(".tmp"):
                raise OSError("simulated owner open failure")
            return original_open(path, *args, **kwargs)

        with patch.object(Path, "open", new=refuse_owner_open):
            with self.assertRaisesRegex(OSError, "owner open failure"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertFalse((self.output / self.run_id / "qualification-result.json").exists())
        self.assertEqual(json.loads(self.receipts["release_receipt"].read_text())[
            "qualification_outcome"], "pending")
        with qualification.executor._HostLock(self.lease):
            pass

    def test_operational_receipts_never_assert_qualification_success(self) -> None:
        self.invoke(lambda *_args, **_kwargs: {
            "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
        })
        final = json.loads((self.output / self.run_id / "qualification-result.json").read_text())
        self.assertEqual(final["status"], "tooling-executed-nonpromoting")
        for path in (self.receipts["expiry_receipt"], self.receipts["release_receipt"]):
            receipt = json.loads(path.read_text())
            self.assertNotEqual(receipt.get("terminal_status"), "complete")
            self.assertEqual(receipt.get("qualification_outcome"), "pending")
        self.assertEqual(json.loads(self.receipts["owner_receipt"].read_text())[
            "qualification_outcome"], "pending")
        for name in ("heartbeat_receipt", "handoff_receipt"):
            for line in self.receipts[name].read_text().splitlines():
                self.assertEqual(json.loads(line)["qualification_outcome"], "pending")
        self.assertEqual(json.loads((self.row_output / "transition-result.json").read_text())[
            "qualification_outcome"], "pending")

    def test_final_result_binds_exact_operational_receipts(self) -> None:
        self.invoke(lambda *_args, **_kwargs: {
            "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
        })
        final = json.loads((self.output / self.run_id / "qualification-result.json").read_text())
        self.assertEqual(final["run_id"], self.run_id)
        self.assertLess(final["deadline_observed_at"], final["lease_expires_at"])
        manifest = final["receipt_manifest"]
        for name, path in self.receipts.items():
            self.assertEqual(manifest[name], {
                "path": str(path.resolve()), "sha256": qualification._sha_file(path),
            })
        row_result = self.row_output / "transition-result.json"
        self.assertEqual(manifest["row_results"], [{
            "path": str(row_result.resolve()), "sha256": qualification._sha_file(row_result),
        }])
        self.assertEqual(qualification.load_committed_result(
            self.output / self.run_id / "qualification-result.json"
        )["run_id"], self.run_id)

    def test_final_marker_is_not_accepted_after_bound_receipt_changes(self) -> None:
        self.invoke(lambda *_args, **_kwargs: {
            "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
        })
        final_path = self.output / self.run_id / "qualification-result.json"
        with self.receipts["release_receipt"].open("a", encoding="utf-8") as stream:
            stream.write(" ")
        with self.assertRaisesRegex(qualification.Refused, "receipt.*digest"):
            qualification.load_committed_result(final_path)

    def test_marker_cannot_make_a_complete_operational_receipt_authoritative(self) -> None:
        self.invoke(lambda *_args, **_kwargs: {
            "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
        })
        release_path = self.receipts["release_receipt"]
        release = json.loads(release_path.read_text())
        release["terminal_status"] = "complete"
        release_path.write_text(json.dumps(release))
        final_path = self.output / self.run_id / "qualification-result.json"
        final = json.loads(final_path.read_text())
        final["receipt_manifest"]["release_receipt"]["sha256"] = qualification._sha_file(
            release_path
        )
        final_path.write_text(json.dumps(final))
        with self.assertRaisesRegex(qualification.Refused, "cleanup or deadline"):
            qualification.load_committed_result(final_path)

    def test_failed_final_publication_leaves_only_pending_operational_receipts(self) -> None:
        final_path = self.output / self.run_id / "qualification-result.json"
        original_write = qualification._atomic_json

        def refuse_final(target, value):
            if target == final_path:
                raise OSError("simulated final publication failure")
            return original_write(target, value)

        with patch.object(qualification, "_atomic_json", side_effect=refuse_final):
            with self.assertRaisesRegex(OSError, "final publication failure"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertFalse(final_path.exists())
        for path in (self.receipts["expiry_receipt"], self.receipts["release_receipt"]):
            receipt = json.loads(path.read_text())
            self.assertEqual(receipt.get("qualification_outcome"), "pending")
            self.assertNotEqual(receipt.get("terminal_status"), "complete")

    def test_interrupt_before_final_commit_cannot_promote_operational_receipts(self) -> None:
        final_path = self.output / self.run_id / "qualification-result.json"
        original_write = qualification._atomic_json

        def interrupt_before_replace(target, value):
            if target == final_path:
                raise KeyboardInterrupt("simulated pre-commit interrupt")
            return original_write(target, value)

        with patch.object(qualification, "_atomic_json", side_effect=interrupt_before_replace):
            with self.assertRaisesRegex(KeyboardInterrupt, "pre-commit interrupt"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertFalse(final_path.exists())
        for path in (self.receipts["expiry_receipt"], self.receipts["release_receipt"]):
            self.assertEqual(json.loads(path.read_text())["qualification_outcome"], "pending")

    def test_uncertain_final_publication_inspects_committed_marker(self) -> None:
        final_path = self.output / self.run_id / "qualification-result.json"
        original_write = qualification._atomic_json

        def committed_then_error(target, value):
            original_write(target, value)
            if target == final_path:
                raise OSError("simulated error after final replacement")

        with patch.object(qualification, "_atomic_json", side_effect=committed_then_error):
            result, _configs = self.invoke(lambda *_args, **_kwargs: {
                "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
            })
        self.assertEqual(result["status"], "tooling-executed-nonpromoting")
        self.assertTrue(final_path.is_file())

    def test_unreadable_post_replace_marker_reports_unknown_not_refused(self) -> None:
        final_path = self.output / self.run_id / "qualification-result.json"
        original_write = qualification._atomic_json

        def damaged_after_replace(target, value):
            original_write(target, value)
            if target == final_path:
                target.write_text("{damaged")
                raise OSError("simulated error after damaged replacement")

        with patch.object(qualification, "_atomic_json", side_effect=damaged_after_replace):
            with self.assertRaisesRegex(qualification.OutcomeUnknown, "publication outcome uncertain"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertTrue(final_path.is_file())

    def test_postpublication_validation_io_failure_reports_unknown(self) -> None:
        final_path = self.output / self.run_id / "qualification-result.json"
        with patch.object(qualification, "load_committed_result",
                          side_effect=OSError("simulated marker read failure")):
            with self.assertRaisesRegex(qualification.OutcomeUnknown, "postpublication"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertTrue(final_path.is_file())

    def test_postpublication_validation_must_match_this_invocation(self) -> None:
        final_path = self.output / self.run_id / "qualification-result.json"
        with patch.object(qualification, "load_committed_result",
                          return_value={"run_id": self.run_id, "status": "other-result"}):
            with self.assertRaisesRegex(qualification.OutcomeUnknown, "marker differs"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertTrue(final_path.is_file())


    def test_deadline_is_rechecked_after_receipt_binding_before_success(self) -> None:
        original_exit = qualification.TransitionLease.__exit__
        original_sha = qualification._sha_file
        released_lease = []
        row_result = self.row_output / "transition-result.json"

        def capture_exit(lease, *args):
            released_lease.append(lease)
            return original_exit(lease, *args)

        def expire_after_binding(path):
            digest = original_sha(path)
            if path == row_result:
                released_lease[0].expires_at = 0
            return digest

        with patch.object(qualification.TransitionLease, "__exit__", capture_exit), \
             patch.object(qualification, "_sha_file", side_effect=expire_after_binding):
            with self.assertRaisesRegex(qualification.Refused, "lease expired"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })
        self.assertFalse((self.output / self.run_id / "qualification-result.json").exists())

    def test_failed_expiry_correction_cannot_leave_authoritative_success(self) -> None:
        original_exit = qualification.TransitionLease.__exit__
        original_write = qualification._atomic_json
        active_lease = []

        def capture_exit(lease, *args):
            active_lease.append(lease)
            return original_exit(lease, *args)

        def expire_then_refuse_correction(target, value):
            if target.name.endswith(".expired"):
                raise OSError("simulated ENOSPC correcting expired receipt")
            original_write(target, value)
            if target == self.receipts["release_receipt"]:
                active_lease[0].expires_at = 0

        with patch.object(qualification.TransitionLease, "__exit__", capture_exit), \
             patch.object(qualification, "_atomic_json", side_effect=expire_then_refuse_correction):
            with self.assertRaisesRegex(qualification.Refused, "lease expired|cleanup failed"):
                self.invoke(lambda *_args, **_kwargs: {
                    "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
                })

        self.assertFalse((self.output / self.run_id / "qualification-result.json").exists())
        for path in (self.receipts["expiry_receipt"], self.receipts["release_receipt"]):
            receipt = json.loads(path.read_text())
            self.assertNotEqual(receipt.get("terminal_status"), "complete")
            self.assertEqual(receipt.get("qualification_outcome"), "pending")

    def test_final_lease_expiry_refuses_success_and_releases_lock(self) -> None:
        self._check_terminal_cleanup(expire="before")

    def test_expiry_during_cleanup_receipts_refuses_success(self) -> None:
        self._check_terminal_cleanup(expire="during")

    def test_final_expiry_preserves_existing_execution_exception(self) -> None:
        self._check_terminal_cleanup(expire="before", fail=True)

    def test_successful_final_cleanup_records_pending_and_releases_lock(self) -> None:
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
        # The expiry receipt reports its own observation; a deadline crossed
        # later during release I/O is refused at the final commit boundary.
        self.assertEqual(expiry["expiry_status"],
                         "expired" if expire == "before" else "not-expired")
        for receipt in (expiry, release):
            self.assertEqual(receipt["terminal_status"],
                             "failed" if expire == "before" or fail else "body-validated")
            self.assertEqual(receipt["qualification_outcome"], "pending")
        self.assertEqual(release["release_status"], "released")
        self.assertTrue(release["flock_release_confirmed"])
        self.assertIsNone(captured[0][0].file)
        self.assertTrue(captured[0][1].closed)
        with qualification.executor._HostLock(self.lease):
            pass

    def test_stale_final_marker_is_refused_before_lease(self) -> None:
        final_path = self.output / self.run_id / "qualification-result.json"
        final_path.parent.mkdir(parents=True)
        final_path.write_text("{}")
        execute = unittest.mock.Mock()
        with self.assertRaisesRegex(qualification.Refused, "stale.*qualification-result"):
            self.invoke(execute)
        execute.assert_not_called()
        self.assertFalse(self.receipts["owner_receipt"].exists())
        self.assertEqual(final_path.read_text(), "{}")

    def test_caller_repository_must_be_this_entry_points_checkout(self) -> None:
        execute = unittest.mock.Mock()
        with TemporaryDirectory() as other:
            with self.assertRaisesRegex(qualification.Refused, "checkout containing this entry point"):
                self.invoke(execute, repository=Path(other))
        execute.assert_not_called()
        self.assertFalse(self.receipts["owner_receipt"].exists())

    def test_matrix_must_be_the_callers_canonical_matrix_and_is_bound(self) -> None:
        execute = unittest.mock.Mock()
        copy = self.root / "version_transition_matrix.json"
        copy.write_bytes(qualification.CANONICAL_MATRIX.read_bytes())
        with self.assertRaisesRegex(qualification.Refused, "canonical transition matrix"):
            self.invoke(execute, matrix_path=copy)
        execute.assert_not_called()
        self.assertFalse(self.receipts["owner_receipt"].exists())
        result, _configs = self.invoke(lambda *_args, **_kwargs: {
            "phase": "complete", "scope": {"write_acceptance": "passed_per_release"},
        })
        expected = {"path": str(qualification.CANONICAL_MATRIX),
                    "sha256": qualification._sha_file(qualification.CANONICAL_MATRIX)}
        self.assertEqual(result["matrix"], expected)
        final_path = self.output / self.run_id / "qualification-result.json"
        self.assertEqual(json.loads(final_path.read_text())["matrix"], expected)
        broken = json.loads(final_path.read_text())
        broken["matrix"]["sha256"] = "0" * 64
        final_path.write_text(json.dumps(broken))
        with self.assertRaisesRegex(qualification.Refused, "matrix"):
            qualification.load_committed_result(final_path)

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
        self.assertIsInstance(recover.call_args.kwargs["host_lock_fd"], qualification.executor._HeldHostLock)
        self.assertEqual(getattr(caught.exception, "_oxidex_durable_recovery"), "recovered")
        self.assertEqual(json.loads(self.receipts["release_receipt"].read_text())["terminal_status"], "failed")

    def test_real_child_interrupt_is_reaped_before_verified_durable_recovery(self) -> None:
        """Non-catchable cleanup proves exit before publishing durable recovery."""
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

        def recover(run_dir, _archive_cache, _source_root, **kwargs):
            self.assertIsInstance(kwargs["host_lock_fd"], qualification.executor._HeldHostLock)
            for name in ("direct", "descendant"):
                if qualification.executor._pid_live(process_ids[name]):
                    raise qualification.executor.Refused(f"owned {name} is still live")
            journal_path = run_dir / "execution-status.json"
            recovered = json.loads(journal_path.read_text())
            recovered["phase"], recovered["active"] = "interrupted", None
            journal_path.write_text(json.dumps(recovered))
            return recovered

        try:
            with patch.object(qualification.executor, "recover", side_effect=recover) as recovery:
                with self.assertRaises(KeyboardInterrupt) as caught:
                    self.invoke(execute)
            recovery.assert_called_once()
            self.assertEqual(getattr(caught.exception, "_oxidex_durable_recovery", None), "recovered")
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


class FailClosedLeaseTests(unittest.TestCase):
    """A lease is released only after every owned child is proven gone."""

    def test_interrupted_live_child_keeps_lease_from_independent_contender(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            lease = root / "output" / "transition.host.lock"
            child_file = root / "child.pid"
            entry = (f"import sys; sys.path.insert(0, {str(HERE)!r}); "
                     "import test_version_transition_qualification as t; "
                     f"raise SystemExit(t._interrupted_live_child_wrapper({str(root)!r}))")
            wrapper = subprocess.Popen([sys.executable, "-c", entry], cwd=root, text=True,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.addCleanup(lambda: wrapper.poll() is None and (wrapper.kill(), wrapper.wait(10)))
            deadline = time.monotonic() + 60
            while not child_file.is_file() and wrapper.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            if not child_file.is_file():
                wrapper.kill()
                _stdout, stderr = wrapper.communicate(timeout=10)
                self.fail(f"wrapper never spawned its stage child: {stderr}")
            time.sleep(0.05)
            child_pid = int(child_file.read_text())
            self.addCleanup(_kill_own_session_child, child_pid)

            os.kill(wrapper.pid, signal.SIGINT)
            _stdout, stderr = wrapper.communicate(timeout=30)

            self.assertTrue(_pid_live(child_pid), "the stage child must outlive the interrupted wrapper")
            self.assertEqual(
                _contend(lease), "blocked",
                f"independent contender acquired the transition lease while stage child "
                f"PID {child_pid} is still live; wrapper exit {wrapper.returncode}, stderr:\n{stderr}",
            )
            self.assertEqual(wrapper.returncode, LIVE_CHILD_EXIT, stderr)
            self.assertIn(f"PID {child_pid}", stderr)
            self.assertIn("intentionally still held", stderr)
            release = json.loads((root / "output/live-child/lease-release.json").read_text())
            self.assertEqual(release["release_status"], "retained-unproven-child")
            self.assertFalse(release["flock_release_confirmed"])
            self.assertEqual([child["pid"] for child in release["surviving_children"]], [child_pid])
            self.assertEqual(release["qualification_outcome"], "pending")
            self.assertFalse((root / "output/live-child/qualification-result.json").exists())

            # Proof that the child is gone is the only thing that frees the lease.
            _kill_own_session_child(child_pid)
            deadline = time.monotonic() + 10
            while _contend(lease) != "acquired":
                if time.monotonic() >= deadline:
                    self.fail("lease stayed held after the surviving child exited")
                time.sleep(0.05)

    def _lease(self, root: Path) -> qualification.TransitionLease:
        lease_path = root / "transition.host.lock"
        lease_path.touch()
        return qualification.TransitionLease(
            lease=lease_path, run_id="fail-closed", owner_receipt=root / "owner.json",
            heartbeat_receipt=root / "heartbeat.jsonl", expiry_receipt=root / "expiry.json",
            release_receipt=root / "release.json",
        )

    def test_unproven_child_is_neither_unlocked_nor_closed_in_process(self) -> None:
        with TemporaryDirectory() as temporary, \
             patch.object(qualification.executor, "_OWNED", qualification.executor._OwnedChildren()):
            root = Path(temporary)
            lease = self._lease(root)
            child = None
            try:
                with self.assertRaises(qualification.LeaseRetained) as raised:
                    with lease:
                        child = qualification.executor._spawn(
                            [sys.executable, "-c", "import time; time.sleep(60)"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                            start_new_session=True, close_fds=True,
                        )
                        lease.finish("body-validated")
                self.assertIn(f"PID {child.pid}", str(raised.exception))
                self.assertIsNone(lease.file)
                # The child here holds no inherited descriptor: only the
                # parent's deliberately retained descriptor keeps the lease.
                self.assertEqual(_contend(root / "transition.host.lock"), "blocked")
                self.assertTrue(qualification.executor.release_retained_locks())
                self.assertEqual(_contend(root / "transition.host.lock"), "blocked")
            finally:
                if child is not None:
                    child.kill()
                    child.wait(10)
            self.assertEqual(qualification.executor.release_retained_locks(), [])
            self.assertEqual(_contend(root / "transition.host.lock"), "acquired")

    def test_spawn_interrupted_before_pid_is_known_fails_closed(self) -> None:
        owned = qualification.executor._OwnedChildren()
        with TemporaryDirectory() as temporary, \
             patch.object(qualification.executor, "_OWNED", owned):
            root = Path(temporary)
            lease = self._lease(root)
            with patch.object(qualification.executor.subprocess, "Popen",
                              side_effect=KeyboardInterrupt("interrupted inside spawn")):
                with self.assertRaises(qualification.LeaseRetained) as raised:
                    with lease:
                        qualification.executor._spawn([sys.executable, "-c", "pass"])
            self.assertIn("PID unknown", str(raised.exception))
            self.assertIsInstance(raised.exception.__context__, KeyboardInterrupt)
            self.assertEqual(_contend(root / "transition.host.lock"), "blocked")
            owned.spawns_in_flight = 0
            self.assertEqual(qualification.executor.release_retained_locks(), [])
            self.assertEqual(_contend(root / "transition.host.lock"), "acquired")

    def test_reaped_child_releases_the_lease_normally(self) -> None:
        with TemporaryDirectory() as temporary, \
             patch.object(qualification.executor, "_OWNED", qualification.executor._OwnedChildren()):
            root = Path(temporary)
            lease = self._lease(root)
            with lease:
                record = qualification.executor._run_record(
                    [sys.executable, "-c", "pass"], cwd=root, env=dict(os.environ), run=subprocess.run,
                )
                self.assertEqual(record["state"], "ok")
                lease.finish("body-validated")
            release = json.loads((root / "release.json").read_text())
            self.assertEqual(release["release_status"], "released")
            self.assertTrue(release["flock_release_confirmed"])
            self.assertEqual(release["surviving_children"], [])
            self.assertEqual(_contend(root / "transition.host.lock"), "acquired")


class MainOutcomeTests(unittest.TestCase):
    def test_retained_lease_exit_names_survivors_and_is_not_a_refusal(self) -> None:
        args = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--lease", "lease", "--run-id", "retained", "--owner-receipt", "owner",
            "--heartbeat-receipt", "heartbeat", "--expiry-receipt", "expiry",
            "--release-receipt", "release", "--handoff-receipt", "handoff",
        ]
        messages = []

        def reporting(*parts, **_kwargs):
            messages.append(" ".join(str(part) for part in parts))

        retained = qualification.LeaseRetained(
            "transition lease lease is intentionally still held: PID 4242 (process group 4242, running)",
            [{"pid": 4242, "pgid": 4242, "state": "running"}],
        )
        with patch.object(qualification, "run_qualification", side_effect=retained), \
             patch("builtins.print", side_effect=reporting):
            self.assertEqual(qualification.main(args), LIVE_CHILD_EXIT)
        self.assertIn("PID 4242", messages[-1])
        self.assertIn("intentionally still held", messages[-1])
        self.assertNotIn("refused", messages[-1])

    def test_success_prints_instrument_before_status(self) -> None:
        args = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--lease", "lease", "--run-id", "committed", "--owner-receipt", "owner",
            "--heartbeat-receipt", "heartbeat", "--expiry-receipt", "expiry",
            "--release-receipt", "release", "--handoff-receipt", "handoff",
        ]
        instrument = {
            "source_commit": "a" * 40,
            "binary": {"path": "/isolated/oxidex", "sha256": "b" * 64, "bytes": 123},
            "native_identity": {"release": "13.59", "perl": {"path": "/pinned/perl", "sha256": "c" * 64},
                                "source": {"path": "/pinned/exiftool"},
                                "lib": {"path": "/pinned/lib", "exiftool_pm_sha256": "d" * 64}},
            "native_probe_sha256": "e" * 64,
            "read_fixture_manifest": "/pinned/read-fixtures.json",
            "read_fixture_manifest_sha256": "f" * 64,
            "read_fixture_count": 1,
            "capability_probe": {"state": "ready", "version": "13.59", "docx_filetype": "DOCX",
                                 "perl_modules_available": True},
        }
        release_tests = {"commands": [["cargo", "test", "--workspace"]], "exits": [0], "passed": 8,
                         "failed": 0, "ignored": 2, "measured": 0, "filtered_out": 0, "targets": 4,
                         "duration_seconds": 3.0, "target_directory": "/isolated/test-suite",
                         "log": {"path": "/isolated/test-command.json", "sha256": "a" * 64},
                         "exiftool_oracle": {"version": "13.59", "tree_realpath": "/pinned/exiftool",
                                             "docx_filetype": "DOCX"}}
        result = {"run_id": "committed", "status": "tooling-executed-nonpromoting",
                  "promotion": "forbidden",
                  "matrix": {"path": "/checkout/tools/exiftool-tables/version_transition_matrix.json",
                             "sha256": "3" * 64},
                  "caller": {"head": "0" * 40, "pin_version": "13.59", "status": "clean"},
                  "rows": [{"id": "same-pin-13.59",
                            "before": {"release": "13.59", "instrument": instrument, "release_tests": release_tests},
                            "after": {"release": "13.59", "instrument": instrument, "release_tests": release_tests}}]}
        output = io.StringIO()
        with patch.object(qualification, "run_qualification", return_value=result), redirect_stdout(output):
            self.assertEqual(qualification.main(args), 0)
        lines = output.getvalue().splitlines()
        self.assertEqual(lines[0], "=== instrument: version_transition_qualification.py ===")
        self.assertIn("tree clean", lines[1])
        self.assertEqual(lines[2], "matrix:  /checkout/tools/exiftool-tables/version_transition_matrix.json "
                                   "sha256=" + "3" * 64)
        self.assertTrue(any("capability ready -ver=13.59 OOXML.docx=DOCX perl-modules=available" in line
                            for line in lines), lines)
        self.assertTrue(any("tests passed=8 failed=0 ignored=2 targets=4 log=/isolated/test-command.json" in line
                            for line in lines), lines)
        self.assertTrue(any("tests graded by ExifTool 13.59 tree=/pinned/exiftool OOXML.docx=DOCX" in line
                            for line in lines), lines)
        self.assertIn("/isolated/oxidex", output.getvalue())
        self.assertIn("/pinned/exiftool", output.getvalue())
        self.assertIn("/pinned/read-fixtures.json", output.getvalue())
        self.assertEqual(json.loads(lines[-1]), {
            "run_id": "committed", "status": "tooling-executed-nonpromoting", "promotion": "forbidden",
        })

    def test_interrupt_message_does_not_claim_recovery_without_proof(self) -> None:
        args = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--lease", "lease", "--run-id", "interrupted", "--owner-receipt", "owner",
            "--heartbeat-receipt", "heartbeat", "--expiry-receipt", "expiry",
            "--release-receipt", "release", "--handoff-receipt", "handoff",
        ]
        messages = []

        def reporting(*parts, **_kwargs):
            messages.append(" ".join(str(part) for part in parts))

        with patch.object(qualification, "run_qualification", side_effect=KeyboardInterrupt()), \
             patch("builtins.print", side_effect=reporting):
            self.assertEqual(qualification.main(args), 130)
        self.assertIn("inspect", messages[-1])
        self.assertNotIn("after durable recovery", messages[-1])

    def test_unknown_publication_is_not_reported_as_refusal(self) -> None:
        args = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--lease", "lease", "--run-id", "uncertain", "--owner-receipt", "owner",
            "--heartbeat-receipt", "heartbeat", "--expiry-receipt", "expiry",
            "--release-receipt", "release", "--handoff-receipt", "handoff",
        ]
        messages = []

        def reporting(*parts, **_kwargs):
            messages.append(" ".join(str(part) for part in parts))

        with patch.object(qualification, "run_qualification",
                          side_effect=qualification.OutcomeUnknown("marker unreadable")), \
             patch("builtins.print", side_effect=reporting):
            self.assertEqual(qualification.main(args), 4)
        self.assertIn("outcome unknown", messages[-1])
        self.assertNotIn("refused", messages[-1])

    def test_reporting_failure_after_commit_is_not_called_a_refusal(self) -> None:
        args = [
            "--matrix", "matrix", "--repository", "repository", "--output", "output",
            "--lease", "lease", "--run-id", "committed", "--owner-receipt", "owner",
            "--heartbeat-receipt", "heartbeat", "--expiry-receipt", "expiry",
            "--release-receipt", "release", "--handoff-receipt", "handoff",
        ]
        messages = []

        def reporting(*parts, **_kwargs):
            messages.append(" ".join(str(part) for part in parts))
            if len(messages) == 1:
                raise OSError("simulated stdout failure")

        result = {"run_id": "committed", "status": "tooling-executed-nonpromoting",
                  "promotion": "forbidden"}
        with patch.object(qualification, "run_qualification", return_value=result), \
             patch.object(qualification, "_instrument_header", return_value="=== instrument: committed ==="), \
             patch("builtins.print", side_effect=reporting):
            self.assertEqual(qualification.main(args), 3)
        self.assertIn("committed", messages[-1])
        self.assertNotIn("refused", messages[-1])


if __name__ == "__main__":
    unittest.main()
