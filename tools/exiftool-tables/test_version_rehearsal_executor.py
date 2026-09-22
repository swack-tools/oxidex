"""Offline scheduler tests for the non-promoting version rehearsal executor."""
from __future__ import annotations

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
from tempfile import TemporaryDirectory
import threading
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import version_rehearsal_executor as executor
import version_rehearsal_native_oracle as native
import test_version_rehearsal_native_oracle as fixture
import artifacts
import table_modules


def ready_probe(release: str) -> dict:
    payload = {"schema": native.SCHEMA, "kind": native.KIND, "identity": {"release": release},
               "cases": [{"name": "case", "state": "ready"}], "state": "ready"}
    return {**payload, "probe_sha256": native.catalog_stage.sha256_json(payload)}


def fixture_text(artifact) -> str:
    """Placeholder contents for one declared output. A split table hub names
    its module files, as a generated one does, so the fake checkout is a
    consistent split set (artifacts.family_errors)."""
    if artifact.key in artifacts.MODULE_FAMILIES:
        return table_modules.render_hub(f"//! {artifact.key}\n", artifacts.module_stems(artifact.key), "")
    return artifact.key


class ExecutorTests(unittest.TestCase):
    def setUp(self):
        temporary, capture, catalog, plan, resolution, materialization, cache, sources, _ = fixture.make_state()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.capture, self.catalog, self.plan = capture, catalog, plan
        self.resolution, self.materialization = resolution, materialization
        self.cache, self.sources = cache, sources
        self.repository = self.root / "oxidex-repository"
        (self.repository / ".git").mkdir(parents=True)
        self.run_dir = self.root / "execution"
        self.releases = sorted({side["release"] for pair in plan["pairs"] for side in (pair["old"], pair["new"])})
        self.calls, self.checkouts, self.native_calls = [], [], []
        self.lock = self.root / "shared-host.lock"
        self.fixture = self.root / "fixture.jpg"; self.fixture.write_bytes(b"fixture")
        self.fixture_manifest = self.root / "fixtures.json"
        self.fixture_manifest.write_text(json.dumps({
            "schema": 1, "kind": "oxidex_version_rehearsal_fixture_manifest",
            "fixtures": [{"path": str(self.fixture),
                          "sha256": __import__("hashlib").sha256(self.fixture.read_bytes()).hexdigest(),
                          "bytes": self.fixture.stat().st_size}],
        }))
        self.write_fixture = self.root / "write.jpg"; self.write_fixture.write_bytes(b"\xff\xd8fixture")
        self.write_fixture_manifest = self.root / "write-fixtures.json"
        self.write_fixture_manifest.write_text(json.dumps({"schema": 1, "kind": "oxidex_version_rehearsal_write_fixture_manifest", "fixtures": [{"path": str(self.write_fixture), "sha256": __import__("hashlib").sha256(self.write_fixture.read_bytes()).hexdigest(), "bytes": self.write_fixture.stat().st_size}]}))

    def test_timeout_cleanup_oserror_keeps_timeout_identity_and_diagnostic(self):
        """A cleanup fault happens after spawn and must not erase timeout facts."""
        with patch.object(executor, "COMMAND_TIMEOUT_SECONDS", 0.01), \
             patch.object(executor, "_bounded_timeout_cleanup", side_effect=OSError("cleanup bridge failed")):
            record = executor._run_record(
                [sys.executable, "-c", "import time; time.sleep(30)"], cwd=self.root,
                env=dict(os.environ), run=subprocess.run,
            )
        self.assertEqual(record["state"], "timeout")
        self.assertIsNone(record["exit"])
        self.assertIsInstance(record["pid"], int)
        self.assertEqual(record["pgid"], record["pid"])
        self.assertEqual(record["cleanup_operation"], "timeout_cleanup")
        self.assertIn("cleanup bridge failed", record["cleanup_error"])

    def test_postspawn_callback_oserror_reaps_owned_child(self):
        """A callback failure follows spawn, so it must not orphan the child."""
        def fail_after_start(_pid, _pgid):
            raise OSError("journal callback failed")
        record = executor._run_record(
            [sys.executable, "-c", "import time; time.sleep(30)"], cwd=self.root,
            env=dict(os.environ), run=subprocess.run, started=fail_after_start,
        )
        self.assertEqual(record["state"], "execution_failed")
        self.assertEqual(record["operation"], "post_spawn")
        self.assertIn("journal callback failed", record["stderr"])
        self.assertEqual(record["pgid"], record["pid"])
        with self.assertRaises(ProcessLookupError):
            os.kill(record["pid"], 0)

    def config(self, *, write=True):
        commands = {stage: {"argv": [stage]} for stage in ("generate", "build", "read")}
        if write:
            commands["write"] = {"argv": ["write"]}
        result = {"schema": executor.SCHEMA, "commands": commands, "host_lock": str(self.lock),
                "execution_source_commit": self.plan["repository_commit"],
                "perls": {release: str(Path(sys.executable).resolve()) for release in self.releases},
                "native_cases": {release: [{"case": release}] for release in self.releases},
                "read_fixture_manifests": {release: str(self.fixture_manifest) for release in self.releases}}
        if write:
            result["write_fixture_manifests"] = {release: str(self.write_fixture_manifest) for release in self.releases}
        return result

    def initialize(self, config):
        return executor.initialize_run(self.run_dir, self.capture, self.catalog, self.plan,
                                       self.resolution, self.materialization, config)

    def checkout(self, repository, commit, destination, run):
        self.checkouts.append((repository, commit, destination))
        destination.mkdir(parents=True)
        for artifact in artifacts.ARTIFACTS:
            path = destination / artifact.path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(fixture_text(artifact))
        return destination

    def command(self, argv, **kwargs):
        if argv[0] == "git":
            return subprocess.CompletedProcess(argv, 0, self.plan["repository_commit"] + "\n", "")
        self.calls.append((argv, kwargs["cwd"], kwargs["env"]["CARGO_TARGET_DIR"]))
        stage = argv[0]
        env = kwargs["env"]
        report = Path(env["OXIDEX_REHEARSAL_REPORT"])
        body = {"schema": executor.SCHEMA, "kind": executor.RESULT_KIND, "stage": stage,
                "release": env["OXIDEX_REHEARSAL_RELEASE"], "state": "passed", "denominator": 3}
        checkout = Path(env["OXIDEX_REHEARSAL_CHECKOUT"])
        generated = []
        for artifact in artifacts.inventory(checkout):
            path = checkout / artifact.path
            generated.append({"path": artifact.path, "sha256": __import__("hashlib").sha256(path.read_bytes()).hexdigest(), "bytes": path.stat().st_size})
        raw = report.parent / "raw" / f"{stage}.json"; raw.parent.mkdir(parents=True, exist_ok=True); raw.write_text("raw")
        body.update(source_commit=env["OXIDEX_REHEARSAL_SOURCE_COMMIT"],
                    source_tree_sha256=executor._source_tree(checkout)["sha256"],
                    generated_artifacts=generated,
                    raw_report={"path": str(raw), "sha256": __import__("hashlib").sha256(raw.read_bytes()).hexdigest()})
        native_lib = Path(env["OXIDEX_REHEARSAL_NATIVE_LIB"])
        native_perl = Path(env["OXIDEX_REHEARSAL_NATIVE_PERL"])
        body["native_identity"] = {"release": env["OXIDEX_REHEARSAL_RELEASE"],
            "perl": {"path": str(native_perl.resolve()), "sha256": __import__("hashlib").sha256(native_perl.read_bytes()).hexdigest()},
            "source": {"path": str(Path(env["OXIDEX_REHEARSAL_NATIVE_SOURCE"]).resolve())},
            "lib": {"path": str(native_lib.resolve()), "exiftool_pm_sha256": __import__("hashlib").sha256((native_lib / "Image/ExifTool.pm").read_bytes()).hexdigest()}}
        binary = Path(env["CARGO_TARGET_DIR"]) / "debug" / "oxidex"; binary.parent.mkdir(parents=True, exist_ok=True); binary.write_bytes(b"binary")
        body["binary"] = {"path": str(binary), "sha256": __import__("hashlib").sha256(binary.read_bytes()).hexdigest(), "bytes": binary.stat().st_size}
        writer = Path(env["CARGO_TARGET_DIR"]) / "debug" / "oxidex-writer"; writer.write_bytes(b"writer")
        body["writer_binary"] = {"path": str(writer), "sha256": __import__("hashlib").sha256(writer.read_bytes()).hexdigest(), "bytes": writer.stat().st_size}
        write = stage == "write"
        fixture = (self.write_fixture if write else self.fixture).resolve()
        manifest = (self.write_fixture_manifest if write else self.fixture_manifest).resolve()
        staged = Path(env["CARGO_TARGET_DIR"]) / ("write-fixtures" if write else "fixtures") / fixture.name
        staged.parent.mkdir(parents=True, exist_ok=True); staged.write_bytes(fixture.read_bytes())
        fixture_sha = __import__("hashlib").sha256(fixture.read_bytes()).hexdigest()
        body["fixtures"] = {"manifest": str(manifest), "manifest_sha256": __import__("hashlib").sha256(manifest.read_bytes()).hexdigest(), "entries": [{"source": str(fixture), "sha256": fixture_sha, "bytes": fixture.stat().st_size, "corpus_path": str(staged), "corpus_sha256": fixture_sha, "corpus_bytes": staged.stat().st_size}]}
        if stage in {"read", "write"}:
            body.update(native_release=env["OXIDEX_REHEARSAL_RELEASE"],
                        native_probe_sha256=ready_probe(env["OXIDEX_REHEARSAL_RELEASE"])["probe_sha256"],
                        comparison={"kind": "oxidex_vs_native", "native_release": env["OXIDEX_REHEARSAL_RELEASE"],
                                    "matched": 3, "mismatched": 0})
        if stage == "read":
            body["classification_counts"] = {
                "matched": 3, "value_diff": 0, "missing": 0, "renames": 0, "extra": 0,
            }
        if stage == "write":
            body["write_mode"] = {"kind": "selected-release-live-native", "release": env["OXIDEX_REHEARSAL_RELEASE"],
                                  "ledger_sha256": __import__("hashlib").sha256((checkout / "tools/exiftool-tables/tiff_scalar_final_ledger.json").read_bytes()).hexdigest(),
                                  "rules_sha256": __import__("hashlib").sha256((checkout / "src/writers/generated_tiff_scalar_final_rules.rs").read_bytes()).hexdigest()}
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(body))
        return subprocess.CompletedProcess(argv, 0, "ok", "")

    def probe(self, *args, **kwargs):
        release = args[7]
        self.native_calls.append(release)
        return ready_probe(release)

    def execute(self):
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            return executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                    run=self.command, checkout=self.checkout)

    def test_runs_both_versions_with_owned_targets_and_per_version_oracles(self):
        self.initialize(self.config())
        journal = self.execute()
        self.assertEqual(journal["phase"], "complete")
        self.assertEqual(journal["promotion"], "forbidden")
        self.assertEqual(journal["scope"]["parity"], "per-version-read-write-rehearsed; no-promotion")
        self.assertEqual(self.native_calls, self.releases)
        self.assertEqual(len(self.checkouts), len(self.releases))
        self.assertTrue(all(commit == self.plan["repository_commit"] for _, commit, _ in self.checkouts))
        self.assertEqual({target for _, _, target in self.calls},
                         {str(self.run_dir / "targets" / executor._safe_name(release)) for release in self.releases})
        for release in self.releases:
            self.assertEqual(journal["releases"][release]["state"], "passed")
            self.assertTrue(all(value == "passed" for value in journal["releases"][release]["stages"].values()))
            self.assertEqual(journal["releases"][release]["reports"]["read"]["denominator"], 3)
            self.assertEqual(journal["releases"][release]["reports"]["write"]["denominator"], 3)
            command_log = self.run_dir / journal["releases"][release]["reports"]["build"]["command"]["path"]
            self.assertEqual(json.loads(command_log.read_text())["stdout"], "ok")

    def test_selected_release_uses_one_verified_plan_side_and_explicit_target(self):
        selected = self.releases[0]
        config = self.config()
        config["execution_releases"] = [selected]
        config["perls"] = {selected: config["perls"][selected]}
        config["native_cases"] = {selected: config["native_cases"][selected]}
        config["read_fixture_manifests"] = {
            selected: config["read_fixture_manifests"][selected],
        }
        config["write_fixture_manifests"] = {
            selected: config["write_fixture_manifests"][selected],
        }
        explicit_target = self.root / "assigned-target" / selected
        config["target_directories"] = {selected: str(explicit_target)}
        self.initialize(config)
        journal = self.execute()
        self.assertEqual(set(journal["releases"]), {selected})
        self.assertEqual(self.native_calls, [selected])
        self.assertEqual({target for _, _, target in self.calls}, {str(explicit_target)})
        self.assertTrue(explicit_target.is_dir())

    def test_absent_write_command_is_visible_not_parity(self):
        self.initialize(self.config(write=False))
        journal = self.execute()
        self.assertEqual(journal["phase"], "complete")
        self.assertEqual(journal["scope"]["write_acceptance"], "unsupported_for_one_or_more_releases")
        self.assertEqual(journal["scope"]["parity"], "unproven_without_all_per-release_read_and_write_acceptance")
        self.assertTrue(all(row["stages"]["write"] == "unsupported" for row in journal["releases"].values()))
        self.assertNotIn("write", [argv[0] for argv, _, _ in self.calls])

    def test_missing_denominator_fails_instead_of_counting_a_build_as_parity(self):
        self.initialize(self.config())
        original = self.command
        def no_denominator(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "read":
                report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                body = json.loads(report.read_text()); body["denominator"] = 0; report.write_text(json.dumps(body))
            return result
        self.command = no_denominator
        journal = self.execute()
        self.assertEqual(journal["phase"], "failed")
        failed = next(row for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failed["state"], "failed")
        self.assertEqual(failed["failure"]["stage"], "read")
        self.assertIn("positive denominator", failed["failure"]["detail"])

    def test_native_timeout_persists_terminal_release_failure(self):
        self.initialize(self.config())
        timeout = subprocess.TimeoutExpired(["native"], 1)
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=timeout):
            journal = executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                       run=self.command, checkout=self.checkout)
        release = self.releases[0]
        self.assertEqual(journal["phase"], "failed")
        self.assertEqual(journal["releases"][release]["state"], "failed")
        self.assertEqual(journal["releases"][release]["stages"]["native"], "failed")
        persisted = json.loads((self.run_dir / "execution-status.json").read_text())
        self.assertEqual(persisted["releases"][release]["state"], "failed")

    def test_real_native_child_interrupt_reaps_owned_group_before_recovery(self):
        """The native runner owns the same interruption cleanup boundary."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        child_program = (
            "import json, os, subprocess, sys\n"
            "descendant = subprocess.Popen([sys.executable, '-c', "
            "'import time; time.sleep(60)'])\n"
            "os.write(int(sys.argv[1]), (json.dumps({'descendant': descendant.pid}) + '\\n').encode())\n"
            "raise SystemExit(descendant.wait())\n"
        )

        def interrupted_probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", child_program, str(write_fd)],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=60,
            )
            self.fail("real native child unexpectedly returned after supervisor interruption")

        original_store = executor._store_journal

        def interrupt_when_child_is_recorded(run_dir, value) -> None:
            original_store(run_dir, value)
            active = value.get("active")
            child = active.get("child") if isinstance(active, dict) else None
            if not isinstance(child, dict) or process_ids:
                return
            process_ids.update(direct=child["pid"], pgid=child["pgid"])
            os.close(write_fd)
            payload = b""
            while not payload.endswith(b"\n"):
                chunk = os.read(read_fd, 4096)
                if not chunk:
                    raise AssertionError("native child closed readiness pipe before reporting its descendant")
                payload += chunk
            process_ids.update(json.loads(payload))
            raise KeyboardInterrupt("interrupt during native child journal record")

        try:
            with executor._HostLock(self.lock) as held, \
                 patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=interrupted_probe), \
                 patch.object(executor, "_store_journal", side_effect=interrupt_when_child_is_recorded):
                with self.assertRaisesRegex(KeyboardInterrupt, "native child journal record"):
                    executor._run_native(
                        self.run_dir, journal, release, docs, config,
                        self.cache, self.sources, subprocess.run,
                    )
                for name in ("direct", "descendant"):
                    with self.subTest(process=name), self.assertRaises(ProcessLookupError):
                        os.kill(process_ids[name], 0)
                recovered = executor.recover(
                    self.run_dir, self.cache, self.sources, host_lock_fd=held.file.fileno(),
                )
                self.assertEqual(recovered["phase"], "interrupted")
                self.assertIsNone(recovered["active"])
            with executor._HostLock(self.lock):
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

    def test_timeout_cleanup_normalizes_byte_output_before_journal_rendering(self):
        self.assertEqual(executor._text_output(b"stdout\xff"), "stdout�")
        self.assertEqual(executor._text_output(b""), "")

    def test_mismatches_and_boolean_counts_refuse_pass_results(self):
        path = self.root / "result.json"
        base = {"schema": executor.SCHEMA, "kind": executor.RESULT_KIND, "stage": "read", "release": self.releases[0],
                "state": "passed", "denominator": 3, "native_release": self.releases[0], "native_probe_sha256": "a" * 64,
                "comparison": {"kind": "oxidex_vs_native", "native_release": self.releases[0], "matched": 2, "mismatched": 1}}
        path.write_text(json.dumps(base))
        with self.assertRaisesRegex(executor.Refused, "outcomes"):
            executor._stage_result(path, self.releases[0], "read", "a" * 64)
        base["comparison"] = {"kind": "oxidex_vs_native", "native_release": self.releases[0], "matched": True, "mismatched": 0}
        path.write_text(json.dumps(base))
        with self.assertRaisesRegex(executor.Refused, "outcomes"):
            executor._stage_result(path, self.releases[0], "read", "a" * 64)
        base["comparison"] = {"kind": "oxidex_vs_native", "native_release": self.releases[0], "matched": 1, "mismatched": 0}
        base["denominator"] = True
        path.write_text(json.dumps(base))
        with self.assertRaisesRegex(executor.Refused, "positive denominator"):
            executor._stage_result(path, self.releases[0], "read", "a" * 64)

    def test_checkout_and_absent_stage_output_record_a_failure_journal(self):
        self.initialize(self.config())
        def broken_checkout(*args):
            raise executor.Refused("checkout broke")
        with self.assertRaisesRegex(executor.Refused, "checkout broke"):
            executor.execute(self.run_dir, self.repository, self.cache, self.sources, run=self.command, checkout=broken_checkout)
        journal = json.loads((self.run_dir / "execution-status.json").read_text())
        self.assertEqual(journal["phase"], "failed")
        self.assertEqual(journal["releases"][self.releases[0]]["failure"]["stage"], "checkout")

        self.run_dir = self.root / "missing-output"
        self.initialize(self.config())
        def no_output(argv, **kwargs):
            if argv[0] == "git": return self.command(argv, **kwargs)
            if argv[0] == "build": return subprocess.CompletedProcess(argv, 0, "built", "")
            return self.command(argv, **kwargs)
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            journal = executor.execute(self.run_dir, self.repository, self.cache, self.sources, run=no_output, checkout=self.checkout)
        self.assertEqual(journal["phase"], "failed")
        self.assertEqual(next(row for row in journal["releases"].values() if row["failure"])["failure"]["stage"], "build")

    def test_shared_configured_lock_contends_across_different_run_parents(self):
        self.initialize(self.config())
        other = self.root / "other-parent" / "execution"
        executor.initialize_run(other, self.capture, self.catalog, self.plan, self.resolution, self.materialization, self.config())
        with executor._HostLock(self.lock):
            with self.assertRaisesRegex(executor.Refused, "host lock"):
                executor.execute(other, self.repository, self.cache, self.sources, run=self.command, checkout=self.checkout)

    def test_external_lock_descriptor_must_be_the_held_configured_lease(self):
        self.initialize(self.config())
        self.lock.touch()
        unrelated = self.root / "unrelated.lock"
        unrelated.touch()
        with unrelated.open("r+") as stream:
            with self.assertRaisesRegex(executor.Refused, "differs from configured lease"):
                executor.execute(
                    self.run_dir, self.repository, self.cache, self.sources,
                    run=self.command, checkout=self.checkout, host_lock_fd=stream.fileno(),
                )
        with executor._HostLock(self.lock) as held, \
             patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            journal = executor.execute(
                self.run_dir, self.repository, self.cache, self.sources,
                run=self.command, checkout=self.checkout, host_lock_fd=held.file.fileno(),
            )
        self.assertEqual(journal["phase"], "complete")

    def test_live_child_cannot_be_recovered_as_interrupted(self):
        self.initialize(self.config())
        status = self.run_dir / "execution-status.json"
        journal = json.loads(status.read_text())
        journal["phase"] = "running"
        journal["active"] = {"release": self.releases[0], "stage": "generate", "child": {"pid": os.getpid(), "pgid": os.getpid()}}
        journal["releases"][self.releases[0]]["stages"]["generate"] = "running"
        status.write_text(json.dumps(journal))
        self.lock.touch()
        with executor._HostLock(self.lock) as held, self.assertRaisesRegex(executor.Refused, "still live"):
            executor.recover(self.run_dir, self.cache, self.sources, host_lock_fd=held.file.fileno())

    def test_real_child_interrupt_reaps_owned_group_before_durable_recovery(self):
        """Removing interrupt cleanup must leave the recorded real child live."""
        self.initialize(self.config())
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        ready_error: list[BaseException] = []
        release = self.releases[0]
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

        def started(pid: int, pgid: int) -> None:
            process_ids.update(direct=pid, pgid=pgid)
            journal_path = self.run_dir / "execution-status.json"
            journal = json.loads(journal_path.read_text())
            journal["phase"] = "running"
            journal["active"] = {
                "release": release, "stage": "generate", "child": {"pid": pid, "pgid": pgid},
            }
            journal["releases"][release]["stages"]["generate"] = "running"
            executor._store_journal(self.run_dir, journal)
            os.close(write_fd)

        interrupter = threading.Thread(target=interrupt_when_child_group_is_ready, daemon=True)
        try:
            with executor._HostLock(self.lock) as held:
                interrupter.start()
                with self.assertRaises(KeyboardInterrupt):
                    executor._run_record(
                        [sys.executable, "-c", child_program, str(write_fd)],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run, started=started,
                    )
                interrupter.join(timeout=5)
                self.assertFalse(interrupter.is_alive(), "readiness thread did not observe the real child")
                if ready_error:
                    raise ready_error[0]
                for name in ("direct", "descendant"):
                    with self.subTest(process=name), self.assertRaises(ProcessLookupError):
                        os.kill(process_ids[name], 0)
                recovered = executor.recover(
                    self.run_dir, self.cache, self.sources, host_lock_fd=held.file.fileno(),
                )
                self.assertEqual(recovered["phase"], "interrupted")
                self.assertIsNone(recovered["active"])
            with executor._HostLock(self.lock):
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

    def test_interrupt_cleanup_failure_preserves_original_and_reaps_emergency_child(self):
        child_pid = None

        def interrupted_after_start(pid, _pgid):
            nonlocal child_pid
            child_pid = pid
            raise KeyboardInterrupt("original interruption")

        with patch.object(executor, "_bounded_timeout_cleanup",
                          side_effect=OSError("bounded cleanup failed")):
            with self.assertRaisesRegex(KeyboardInterrupt, "original interruption") as caught:
                executor._run_record(
                    [sys.executable, "-c", "import time; time.sleep(60)"],
                    cwd=self.root, env=dict(os.environ), run=subprocess.run,
                    started=interrupted_after_start,
                )
        self.assertTrue(any("bounded cleanup failed" in note
                            for note in getattr(caught.exception, "__notes__", [])))
        self.assertIsNotNone(child_pid)
        with self.assertRaises(ProcessLookupError):
            os.kill(child_pid, 0)

    def test_interrupt_surviving_group_triggers_emergency_and_blocks_recovery(self):
        """A reaped leader must not hide its still-live owned process group."""
        self.initialize(self.config())
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        release = self.releases[0]
        child_program = (
            "import json, os, subprocess, sys\n"
            "descendant = subprocess.Popen([sys.executable, '-c', "
            "'import time; time.sleep(60)'], stdin=subprocess.DEVNULL, "
            "stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n"
            "os.write(int(sys.argv[1]), (json.dumps({'descendant': descendant.pid}) + '\\n').encode())\n"
            "raise SystemExit(descendant.wait())\n"
        )

        def interrupted_after_group_ready(pid: int, pgid: int) -> None:
            process_ids.update(direct=pid, pgid=pgid)
            journal_path = self.run_dir / "execution-status.json"
            journal = json.loads(journal_path.read_text())
            journal["phase"] = "running"
            journal["active"] = {
                "release": release, "stage": "generate", "child": {"pid": pid, "pgid": pgid},
            }
            journal["releases"][release]["stages"]["generate"] = "running"
            executor._store_journal(self.run_dir, journal)
            os.close(write_fd)
            payload = b""
            while not payload.endswith(b"\n"):
                chunk = os.read(read_fd, 4096)
                if not chunk:
                    raise AssertionError("owned child closed readiness pipe before reporting its descendant")
                payload += chunk
            process_ids.update(json.loads(payload))
            raise KeyboardInterrupt("original interruption")

        def reap_direct_only(child):
            child.terminate()
            return child.communicate(timeout=5)

        try:
            with executor._HostLock(self.lock) as held, \
                 patch.object(executor, "_bounded_timeout_cleanup", side_effect=reap_direct_only), \
                 patch.object(executor, "_emergency_reap_group",
                              side_effect=OSError("emergency cleanup failed")) as emergency:
                with self.assertRaisesRegex(KeyboardInterrupt, "original interruption") as caught:
                    executor._run_record(
                        [sys.executable, "-c", child_program, str(write_fd)],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run,
                        started=interrupted_after_group_ready,
                    )
                notes = list(getattr(caught.exception, "__notes__", []))
                self.assertTrue(any("process group is still live" in note for note in notes))
                with self.assertRaisesRegex(executor.Refused, "process group is still live"):
                    executor.recover(
                        self.run_dir, self.cache, self.sources, host_lock_fd=held.file.fileno(),
                    )
                self.assertTrue(any("emergency cleanup failed" in note for note in notes))
                emergency.assert_called_once()
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

    def test_recovery_accepts_legacy_schema_one_config_without_read_bindings(self):
        self.initialize(self.config())
        config_path = self.run_dir / "inputs/config.json"
        config = json.loads(config_path.read_text())
        config.pop("read_fixture_manifests")
        config.pop("read_fixture_bindings")
        config_path.write_text(json.dumps(config))
        journal_path = self.run_dir / "execution-status.json"
        journal = json.loads(journal_path.read_text())
        release = self.releases[0]
        journal["config_sha256"] = executor._sha_json(config)
        journal["phase"] = "running"
        journal["active"] = {"release": release, "stage": "generate"}
        journal["releases"][release]["stages"]["generate"] = "running"
        journal_path.write_text(json.dumps(journal))
        with self.assertRaisesRegex(executor.Refused, "read command requires"):
            executor._load_journal(self.run_dir, self.cache, self.sources)
        recovered = executor.recover(self.run_dir, self.cache, self.sources)
        self.assertEqual(recovered["phase"], "interrupted")
        self.assertEqual(recovered["releases"][release]["stages"]["generate"], "interrupted")

    def test_stage_guard_failure_after_generate_stops_later_stages(self):
        self.initialize(self.config())
        boundaries = []
        def guard(release, stage, boundary):
            boundaries.append((release, stage, boundary))
            if stage == "generate" and boundary == "after":
                raise executor.Refused("lease receipt guard failed")
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            journal = executor.execute(
                self.run_dir, self.repository, self.cache, self.sources,
                run=self.command, checkout=self.checkout, stage_guard=guard,
            )
        self.assertEqual(journal["phase"], "failed")
        self.assertEqual([argv[0] for argv, _, _ in self.calls], ["generate"])
        self.assertIn((self.releases[0], "native", "before"), boundaries)
        self.assertIn((self.releases[0], "generate", "after"), boundaries)
        self.assertNotIn((self.releases[0], "build", "before"), boundaries)

    def test_main_returns_nonzero_for_failed_execution(self):
        failed = {"phase": "failed", "promotion": "forbidden", "scope": {"parity": "unproven"}}
        with patch.object(executor, "execute", return_value=failed):
            self.assertEqual(executor.main(["execute", "--run-dir", "x", "--repository", "x", "--archive-cache", "x", "--source-root", "x"]), 2)

    def test_interruption_is_durable_and_never_retries_selected_work(self):
        self.initialize(self.config())
        def interrupted(argv, **kwargs):
            if argv[0] == "generate":
                raise KeyboardInterrupt()
            return self.command(argv, **kwargs)
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            with self.assertRaises(KeyboardInterrupt):
                executor.execute(self.run_dir, self.repository, self.cache, self.sources, run=interrupted, checkout=self.checkout)
        before = len(self.calls)
        recovered = executor.recover(self.run_dir, self.cache, self.sources)
        self.assertEqual(recovered["phase"], "interrupted")
        self.assertTrue(any(row["stages"]["generate"] == "interrupted" for row in recovered["releases"].values()))
        with self.assertRaisesRegex(executor.Refused, "terminal"):
            self.execute()
        self.assertEqual(len(self.calls), before)

    def test_mutated_materialized_tree_refuses_before_checkout_or_command(self):
        self.initialize(self.config())
        row = self.materialization["selected_releases"][0]
        (self.sources / row["source_directory"] / "lib/Image/ExifTool.pm").write_text("changed")
        with self.assertRaisesRegex(Exception, "verified archive"):
            self.execute()
        self.assertEqual(self.checkouts, [])
        self.assertEqual(self.calls, [])

    def test_generation_cannot_change_unmanifested_source(self):
        self.initialize(self.config())
        original = self.command
        def mutating(argv, **kwargs):
            if argv[0] == "generate":
                path = Path(kwargs["env"]["OXIDEX_REHEARSAL_CHECKOUT"]) / "src/lib.rs"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("unexpected source mutation")
            return original(argv, **kwargs)
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            journal = executor.execute(self.run_dir, self.repository, self.cache, self.sources, run=mutating, checkout=self.checkout)
        self.assertEqual(journal["phase"], "failed")
        failure = next(row["failure"] for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failure["stage"], "generate")
        self.assertIn("non-generated source", failure["detail"])

    def run_generation_mutating_split_tables(self, mutate):
        self.initialize(self.config())
        original = self.command
        def mutating(argv, **kwargs):
            if argv[0] == "generate":
                mutate(Path(kwargs["env"]["OXIDEX_REHEARSAL_CHECKOUT"]) / "src/exiftool_tables/binary")
            return original(argv, **kwargs)
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            return executor.execute(self.run_dir, self.repository, self.cache, self.sources, run=mutating, checkout=self.checkout)

    def test_generation_may_add_and_drop_split_table_modules_with_their_hub(self):
        # A release with a different ExifTool module set (11.78: no DJI, a JSON module).
        def regenerate(directory):
            stems = sorted((set(table_modules.declared_stems(directory / "mod.rs")) - {"dji"}) | {"json"})
            (directory / "dji.rs").unlink()
            (directory / "json.rs").write_text("json")
            (directory / "mod.rs").write_text(table_modules.render_hub("//! binary\n", stems, ""))
        journal = self.run_generation_mutating_split_tables(regenerate)
        self.assertEqual(journal["phase"], "complete", journal)

    def test_generation_cannot_leave_an_orphan_split_table_module(self):
        journal = self.run_generation_mutating_split_tables(
            lambda directory: (directory / "orphan.rs").write_text("orphan"))
        self.assertEqual(journal["phase"], "failed")
        failure = next(row["failure"] for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failure["stage"], "generate")
        self.assertIn("orphan module file src/exiftool_tables/binary/orphan.rs", failure["detail"])

    def test_write_fixture_manifest_and_jpeg_scope_are_bound_at_init_and_rechecked(self):
        journal = self.initialize(self.config())
        binding = json.loads((self.run_dir / "inputs" / "config.json").read_text())["write_fixture_bindings"]
        self.assertEqual(binding[self.releases[0]]["sha256"], __import__("hashlib").sha256(self.write_fixture_manifest.read_bytes()).hexdigest())
        self.write_fixture.write_bytes(b"changed")
        with self.assertRaisesRegex(executor.Refused, "write fixture"):
            self.execute()
        self.assertEqual(journal["phase"], "planned")

    def test_read_fixture_manifest_and_scope_are_bound_at_init_and_rechecked(self):
        journal = self.initialize(self.config())
        binding = json.loads((self.run_dir / "inputs" / "config.json").read_text())["read_fixture_bindings"]
        self.assertEqual(
            binding[self.releases[0]]["sha256"],
            __import__("hashlib").sha256(self.fixture_manifest.read_bytes()).hexdigest(),
        )
        self.fixture.write_bytes(b"changed")
        with self.assertRaisesRegex(executor.Refused, "read fixture"):
            self.execute()
        self.assertEqual(journal["phase"], "planned")
        self.assertEqual(self.checkouts, [])
        self.assertEqual(self.calls, [])

    def test_read_report_must_cover_exact_bound_fixture_scope(self):
        second = self.root / "second.jpg"
        second.write_bytes(b"second fixture")
        manifest = json.loads(self.fixture_manifest.read_text())
        manifest["fixtures"].append({
            "path": str(second),
            "sha256": __import__("hashlib").sha256(second.read_bytes()).hexdigest(),
            "bytes": second.stat().st_size,
        })
        self.fixture_manifest.write_text(json.dumps(manifest))
        self.initialize(self.config())
        journal = self.execute()
        self.assertEqual(journal["phase"], "failed")
        failure = next(row["failure"] for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failure["stage"], "read")
        self.assertIn("exact immutable selected fixture scope", failure["detail"])

    def test_write_stage_refuses_replaced_writer_binary_or_read_fixture_substitution(self):
        self.initialize(self.config())
        original = self.command
        def replaced_writer(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "write":
                report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                body = json.loads(report.read_text())
                body["writer_binary"] = body["binary"]
                report.write_text(json.dumps(body))
            return result
        self.command = replaced_writer
        journal = self.execute()
        self.assertEqual(journal["phase"], "failed")
        failure = next(row["failure"] for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failure["stage"], "write")
        self.assertIn("writer", failure["detail"])

    def test_write_stage_requires_selected_release_matrix_mode_proof(self):
        self.initialize(self.config())
        original = self.command
        def removes_mode(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "write":
                report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                body = json.loads(report.read_text()); del body["write_mode"]
                report.write_text(json.dumps(body))
            return result
        self.command = removes_mode
        journal = self.execute()
        self.assertEqual(journal["phase"], "failed")
        failure = next(row["failure"] for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failure["stage"], "write")
        self.assertIn("matrix mode", failure["detail"])

    def test_write_stage_rejects_valid_shaped_mode_with_wrong_generated_hashes(self):
        self.initialize(self.config())
        original = self.command
        def wrong_hashes(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "write":
                report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                body = json.loads(report.read_text())
                body["write_mode"]["rules_sha256"] = "0" * 64
                report.write_text(json.dumps(body))
            return result
        self.command = wrong_hashes
        journal = self.execute()
        self.assertEqual(journal["phase"], "failed")
        failure = next(row["failure"] for row in journal["releases"].values() if row["failure"])
        self.assertEqual(failure["stage"], "write")
        self.assertIn("differs from generated source operands", failure["detail"])


if __name__ == "__main__":
    unittest.main()
