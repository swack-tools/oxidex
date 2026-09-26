"""Offline scheduler tests for the non-promoting version rehearsal executor."""
from __future__ import annotations

from contextlib import redirect_stderr
import io
import json
import os
from pathlib import Path
import re
import select
import signal
import subprocess
import sys
from tempfile import TemporaryDirectory
import threading
import time
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import version_rehearsal_executor as executor
import version_rehearsal_native_oracle as native
import version_transition_qualification as qualification
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


CONTENDER = (
    "import fcntl, sys\n"
    "with open(sys.argv[1], 'a+') as stream:\n"
    "    try:\n"
    "        fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)\n"
    "    except BlockingIOError:\n"
    "        print('blocked')\n"
    "    else:\n"
    "        print('acquired')\n"
)


def _contend(lock: Path) -> str:
    """Ask an independent process whether it can take the host lock now."""
    probe = subprocess.run([sys.executable, "-c", CONTENDER, str(lock)],
                           capture_output=True, text=True, timeout=20)
    if probe.returncode != 0:
        raise AssertionError(f"contender probe failed: {probe.stderr}")
    return probe.stdout.strip()


def _read_reported_line(read_fd: int, *, timeout: float = 30.0) -> bytes:
    """Read one newline-terminated report, failing instead of blocking forever.

    A reporting process can die before it writes (a loaded host, or a short
    command timeout killing its parent first); the test must then fail with a
    diagnostic rather than hang the whole suite on the pipe.
    """
    deadline = time.monotonic() + timeout
    payload = b""
    while not payload.endswith(b"\n"):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AssertionError(f"no complete report within {timeout}s (read so far: {payload!r})")
        readable, _, _ = select.select([read_fd], [], [], remaining)
        if not readable:
            continue
        chunk = os.read(read_fd, 4096)
        if not chunk:
            raise AssertionError(f"reporting pipe closed before a complete report (read so far: {payload!r})")
        payload += chunk
    return payload


def _without_lineage_supervisor(test):
    """Exercise the ownership-probe fallback directly on every platform.

    On Linux the subreaper supervisor kills a command's escaped descendants
    before these scenarios can arise; these tests pin the second line of
    defence that still applies when that boundary is absent (Darwin) or
    when cleanup itself faults.
    """
    return patch.object(executor, "_LINEAGE_SUPERVISED", False)(test)


def _isolate_process_ownership(case: unittest.TestCase) -> None:
    """Give each test its own owned-child registry and retained-lock list.

    Both are process-wide by design; a child a test deliberately leaves
    unproven must not make an unrelated later test's lock release fail closed.
    """
    for name, value in (("_OWNED", executor._OwnedChildren()), ("_RETAINED_LOCKS", [])):
        patcher = patch.object(executor, name, value)
        patcher.start()
        case.addCleanup(patcher.stop)


class ExecutorTests(unittest.TestCase):
    def setUp(self):
        _isolate_process_ownership(self)
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

    @_without_lineage_supervisor
    def test_timeout_emergency_failure_refuses_terminal_state_with_escaped_descendant(self):
        """Unverified timeout cleanup cannot escape as a terminal-stage OSError."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        descendant_pid = None
        descendant_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import subprocess, sys, time\n"
            "subprocess.Popen([sys.executable, '-c', sys.argv[2], sys.argv[1]], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
            "time.sleep(60)\n"
        )

        def reap_direct_only(child):
            child.kill()
            child.communicate(timeout=5)
            raise OSError("bounded descendant enumeration unverified")

        def descendant_ready(_pid, _pgid):
            # Synchronize before the command timeout starts: communicate()
            # (and so the 50 ms timeout) begins only after this returns, so a
            # loaded host cannot kill the parent before it launches the
            # descendant. The read is bounded either way.
            nonlocal descendant_pid, write_fd
            os.close(write_fd)
            write_fd = -1
            descendant_pid = int(_read_reported_line(read_fd))

        failure = None
        try:
            with patch.object(executor, "COMMAND_TIMEOUT_SECONDS", 0.05), \
                 patch.object(executor, "_bounded_timeout_cleanup", side_effect=reap_direct_only), \
                 patch.object(executor, "_emergency_reap_group",
                              side_effect=OSError("descendant enumeration unverified")):
                try:
                    executor._run_record(
                        [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run,
                        started=descendant_ready,
                    )
                except BaseException as exc:
                    failure = exc
            if isinstance(failure, AssertionError):
                raise failure
            self.assertIsNotNone(descendant_pid)
            self.assertIsInstance(failure, executor.OwnedChildCleanupIncomplete)
            self.assertIn("cleanup remains incomplete", str(failure))
            self.assertTrue(executor._pid_live(descendant_pid))
        finally:
            for descriptor in (write_fd, read_fd):
                if descriptor < 0:
                    continue
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            if descendant_pid is not None:
                try:
                    os.kill(descendant_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_postspawn_callback_oserror_reaps_owned_child(self):
        """A post-spawn fault non-catchably reaps its verified owned child."""
        child_pid = None

        def fail_after_start(pid, _pgid):
            nonlocal child_pid
            child_pid = pid
            raise OSError("journal callback failed")

        record = executor._run_record(
            [sys.executable, "-c", "import time; time.sleep(30)"], cwd=self.root,
            env=dict(os.environ), run=subprocess.run, started=fail_after_start,
        )
        self.assertEqual(record["state"], "execution_failed")
        self.assertEqual(record["operation"], "post_spawn")
        self.assertNotIn("cleanup_error", record)
        self.assertIsNotNone(child_pid)
        self.assertFalse(executor._pid_live(child_pid))

    @_without_lineage_supervisor
    def test_postspawn_callback_cleanup_failure_is_owned_child_incomplete(self):
        """An unverified post-spawn cleanup cannot degrade to a plain OSError."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        descendant_pid = None
        descendant_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import subprocess, sys, time\n"
            "subprocess.Popen([sys.executable, '-c', sys.argv[2], sys.argv[1]], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
            "time.sleep(60)\n"
        )

        def fail_after_descendant_started(_pid, _pgid):
            nonlocal descendant_pid
            descendant_pid = int(_read_reported_line(read_fd))
            raise OSError("journal persistence failed")

        def reap_direct_only(child):
            child.kill()
            child.communicate(timeout=5)
            raise OSError("bounded descendant verification failed")

        failure = None
        try:
            with patch.object(executor, "_bounded_timeout_cleanup", side_effect=reap_direct_only), \
                 patch.object(executor, "_emergency_reap_group",
                              side_effect=OSError("emergency verification failed")):
                try:
                    executor._run_record(
                        [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run,
                        started=fail_after_descendant_started,
                    )
                except BaseException as exc:
                    failure = exc
            self.assertIsInstance(failure, executor.OwnedChildCleanupIncomplete)
            self.assertIn("cleanup remains incomplete", str(failure))
            self.assertIsNotNone(descendant_pid)
            self.assertTrue(executor._pid_live(descendant_pid))
        finally:
            for descriptor in (write_fd, read_fd):
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            if descendant_pid is not None:
                try:
                    os.kill(descendant_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_stage_preserves_active_child_for_incomplete_postspawn_cleanup(self):
        """The stage journal stays recoverable when post-spawn cleanup is incomplete."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(
            self.run_dir, self.cache, self.sources,
        )
        release = self.releases[0]
        checkout = self.checkout(
            self.repository, self.plan["repository_commit"],
            self.run_dir / "checkouts" / executor._safe_name(release), self.command,
        )
        target = self.run_dir / "targets" / executor._safe_name(release)
        target.mkdir(parents=True)
        child_identity = {"pid": 4242, "pgid": 4242}

        def incomplete_record(*_args, started, **_kwargs):
            started(child_identity["pid"], child_identity["pgid"])
            raise executor.OwnedChildCleanupIncomplete("post-spawn cleanup unverified")

        with patch.object(executor, "_run_record", side_effect=incomplete_record), \
             self.assertRaisesRegex(executor.OwnedChildCleanupIncomplete,
                                    "post-spawn cleanup unverified"):
            executor._run_stage(
                self.run_dir, journal, release, "generate", checkout, target,
                (self.sources / "native", self.sources / "lib", self.sources / "program"),
                sys.executable, ready_probe(release), config, self.command,
            )

        persisted = json.loads((self.run_dir / "execution-status.json").read_text())
        self.assertEqual(persisted["phase"], "running")
        self.assertEqual(persisted["releases"][release]["stages"]["generate"], "running")
        self.assertEqual(persisted["active"], {
            "release": release, "stage": "generate", "child": child_identity,
        })

    def test_postspawn_system_exit_reaps_owned_child_and_preserves_identity(self):
        """SystemExit after child publication is re-raised only after verified cleanup."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        outcome = SystemExit("post-spawn system exit")
        child_identity = None
        failure = None
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen
        child_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), b'ready\\n')\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )

        def exit_after_child_started(pid, pgid):
            nonlocal child_identity
            child_identity = {"pid": pid, "pgid": pgid}
            self.assertEqual(_read_reported_line(read_fd), b"ready\n")
            raise outcome

        def capture_child(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
            return child

        try:
            with patch.object(executor.subprocess, "Popen", side_effect=capture_child):
                try:
                    executor._run_record(
                        [sys.executable, "-c", child_program, str(write_fd)],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run,
                        started=exit_after_child_started,
                    )
                except BaseException as exc:
                    failure = exc
            self.assertIs(failure, outcome)
            self.assertEqual(
                getattr(failure, "_oxidex_owned_child_cleanup", None), "verified",
            )
            self.assertIsNotNone(child_identity)
            self.assertFalse(executor._pid_live(child_identity["pid"]))
            self.assertFalse(executor._group_live(child_identity["pgid"]))
        finally:
            for descriptor in (write_fd, read_fd):
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            if child_identity is not None:
                try:
                    os.killpg(child_identity["pgid"], signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for child in created:
                try:
                    child.communicate(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

    def test_stage_retains_active_child_for_incomplete_system_exit_cleanup(self):
        """Incomplete SystemExit cleanup preserves the original outcome and active journal."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(
            self.run_dir, self.cache, self.sources,
        )
        release = self.releases[0]
        checkout = self.checkout(
            self.repository, self.plan["repository_commit"],
            self.run_dir / "checkouts" / executor._safe_name(release), self.command,
        )
        target = self.run_dir / "targets" / executor._safe_name(release)
        target.mkdir(parents=True)
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        outcome = SystemExit("post-spawn system exit")
        original_store = executor._store_journal
        original_popen = executor.subprocess.Popen
        created: list[subprocess.Popen[str]] = []
        raised = False
        failure = None
        child_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), b'ready\\n')\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        config["commands"]["generate"] = {
            "argv": [sys.executable, "-c", child_program, str(write_fd)],
        }

        def capture_child(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
            return child

        def persist_then_exit(run_dir, current):
            nonlocal raised
            if (not raised and isinstance(current.get("active"), dict)
                    and isinstance(current["active"].get("child"), dict)):
                self.assertEqual(_read_reported_line(read_fd), b"ready\n")
                original_store(run_dir, current)
                raised = True
                raise outcome
            return original_store(run_dir, current)

        try:
            with patch.object(executor.subprocess, "Popen", side_effect=capture_child), \
                 patch.object(executor, "_store_journal", side_effect=persist_then_exit), \
                 patch.object(executor, "_verify_checkout_head"), \
                 patch.object(executor, "_bounded_timeout_cleanup",
                              side_effect=OSError("bounded cleanup failed")), \
                 patch.object(executor, "_emergency_reap_group",
                              side_effect=OSError("emergency cleanup failed")):
                try:
                    executor._run_stage(
                        self.run_dir, journal, release, "generate", checkout, target,
                        (self.sources / "native", self.sources / "lib", self.sources / "program"),
                        sys.executable, ready_probe(release), config, subprocess.run,
                    )
                except BaseException as exc:
                    failure = exc
            self.assertIs(failure, outcome)
            self.assertEqual(
                getattr(failure, "_oxidex_owned_child_cleanup", None), "incomplete",
            )
            self.assertEqual(len(created), 1)
            self.assertTrue(executor._pid_live(created[0].pid))
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "running")
            self.assertEqual(persisted["releases"][release]["stages"]["generate"], "running")
            self.assertEqual(persisted["active"]["child"], {
                "pid": created[0].pid, "pgid": created[0].pid,
            })
        finally:
            for descriptor in (write_fd, read_fd):
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.communicate(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

    @_without_lineage_supervisor
    def test_successful_command_refuses_detached_descendant_retaining_ownership(self):
        """A zero exit cannot become ok while detached owned work retains its proof."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        children: list[subprocess.Popen[str]] = []
        descendant_pid = None
        original_popen = executor.subprocess.Popen
        descendant_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import subprocess, sys\n"
            "subprocess.Popen([sys.executable, '-c', sys.argv[2], sys.argv[1]], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
        )

        def capture_child(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            children.append(child)
            return child

        try:
            failure = None
            with patch.object(executor.subprocess, "Popen", side_effect=capture_child):
                try:
                    executor._run_record(
                        [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run,
                    )
                except BaseException as exc:
                    failure = exc
            descendant_pid = int(_read_reported_line(read_fd))
            self.assertIsInstance(failure, executor.OwnedChildCleanupIncomplete)
            self.assertTrue(executor._pid_live(descendant_pid))
        finally:
            for descriptor in (write_fd, read_fd):
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            if descendant_pid is not None:
                try:
                    os.kill(descendant_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for child in children:
                executor._close_ownership_probe(child)

    def test_owned_command_exit_status_and_signal_are_the_commands(self):
        """The supervisor (Linux) must not replace the command's own status."""
        exited = executor._run_record([sys.executable, "-c", "raise SystemExit(7)"],
                                      cwd=self.root, env=dict(os.environ), run=subprocess.run)
        self.assertEqual((exited["state"], exited["exit"]), ("exit_failed", 7))
        killed = executor._run_record(
            [sys.executable, "-c", "import os, signal; os.kill(os.getpid(), signal.SIGKILL)"],
            cwd=self.root, env=dict(os.environ), run=subprocess.run)
        self.assertEqual((killed["state"], killed["exit"]), ("exit_failed", -signal.SIGKILL))
        ok = executor._run_record([sys.executable, "-c", "print('out')"],
                                  cwd=self.root, env=dict(os.environ), run=subprocess.run)
        self.assertEqual((ok["state"], ok["exit"], ok["stdout"]), ("ok", 0, "out\n"))

    def test_owned_command_exec_failure_is_a_spawn_failure(self):
        """A missing program stays spawn_failed even when a supervisor starts it."""
        missing = self.root / "no-such-owned-program"
        record = executor._run_record([str(missing)], cwd=self.root, env=dict(os.environ),
                                      run=subprocess.run)
        self.assertEqual(record["state"], "spawn_failed", record)
        self.assertIn("No such file", record["stderr"])
        self.assertEqual(executor.unproven_children(), [])

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "the lineage boundary is a Linux child subreaper")
    def test_timeout_sweep_kills_detached_descendant_that_closed_its_descriptors(self):
        """Timeout cleanup reaches a new-session close_fds descendant through the lineage."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        descendant_pid = None
        descendant_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import subprocess, sys, time\n"
            "subprocess.Popen([sys.executable, '-c', sys.argv[2], sys.argv[1]], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=True, pass_fds=(int(sys.argv[1]),))\n"
            "time.sleep(60)\n"
        )

        def descendant_ready(_pid, _pgid):
            nonlocal descendant_pid, write_fd
            os.close(write_fd)
            write_fd = -1
            descendant_pid = int(_read_reported_line(read_fd))

        try:
            with patch.object(executor, "COMMAND_TIMEOUT_SECONDS", 0.05):
                record = executor._run_record(
                    [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                    cwd=self.root, env=dict(os.environ), run=subprocess.run, started=descendant_ready,
                )
            self.assertEqual(record["state"], "timeout", record)
            self.assertNotIn("cleanup_error", record)
            self.assertFalse(executor._pid_live(descendant_pid))
            self.assertEqual(executor.unproven_children(), [])
        finally:
            for descriptor in (write_fd, read_fd):
                if descriptor >= 0:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
            if descendant_pid is not None:
                try:
                    os.kill(descendant_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "the lineage boundary is a Linux child subreaper; Darwin has no kernel "
                         "equivalent, so a descendant that detaches and closes every inherited "
                         "descriptor there remains a documented residual")
    def test_success_cannot_leave_detached_descendant_that_closed_its_descriptors(self):
        """Ownership-pipe EOF is not proof: a detached close_fds descendant must not outlive success."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        descendant_pid = None
        descendant_program = (
            "import os, sys, time\n"
            "for fd in (int(sys.argv[1]), int(sys.argv[2])):\n"
            "    os.write(fd, f'{os.getpid()}\\n'.encode())\n"
            "    os.close(fd)\n"
            "time.sleep(60)\n"
        )
        # The direct child waits until its detached descendant is running,
        # then exits 0. The descendant keeps only the two report pipes it was
        # explicitly passed (close_fds=True): no host-lock or ownership probe.
        child_program = (
            "import os, subprocess, sys\n"
            "ready_read, ready_write = os.pipe()\n"
            "report = int(sys.argv[1])\n"
            "subprocess.Popen([sys.executable, '-c', sys.argv[2], str(report), str(ready_write)], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=True, pass_fds=(report, ready_write))\n"
            "os.close(ready_write)\n"
            "os.read(ready_read, 64)\n"
        )
        try:
            record = executor._run_record(
                [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                cwd=self.root, env=dict(os.environ), run=subprocess.run,
            )
            os.close(write_fd)
            write_fd = -1
            descendant_pid = int(_read_reported_line(read_fd))
            self.assertNotEqual(record["state"], "ok", record)
            self.assertEqual(record["state"], "escaped_descendants", record)
            self.assertIn(descendant_pid, record["escaped_descendants"])
            self.assertEqual(record["exit"], 0)
            self.assertFalse(executor._pid_live(descendant_pid),
                             "a detached descendant outlived the accepted command")
        finally:
            for descriptor in (write_fd, read_fd):
                if descriptor >= 0:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
            if descendant_pid is not None:
                try:
                    os.kill(descendant_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_reaped_leader_group_is_never_signalled_by_numeric_pgid(self):
        """After the leader is reaped its PGID may be reused; signal only verified members."""
        child = executor._spawn([sys.executable, "-c", "pass"], stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
        child.wait(timeout=30)
        member, stranger = 7001, 7002
        setattr(child, "_oxidex_owned_descendants", {member: "identity-owned"})
        identities = {member: "identity-owned", stranger: "identity-stranger"}
        with patch.object(executor.os, "killpg") as killpg, \
             patch.object(executor, "_group_member_pids", return_value=[member, stranger]), \
             patch.object(executor, "_process_identity", side_effect=lambda pid: identities[pid]), \
             patch.object(executor, "_pid_live", return_value=True), \
             patch.object(executor, "_signal_pid") as signal_pid:
            executor._signal_owned_group(child, signal.SIGKILL)
        self.assertNotIn(signal.SIGKILL, [call.args[1] for call in killpg.call_args_list])
        signal_pid.assert_called_once_with(member, signal.SIGKILL)

    def test_probe_and_status_reads_survive_descriptors_above_fd_setsize(self):
        """select() refuses descriptors >= FD_SETSIZE with ValueError; the readers must not."""
        read_fd, write_fd = os.pipe()
        high_read = high_write = -1
        try:
            import resource
            soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
            if hard != resource.RLIM_INFINITY and hard < 1200:
                self.skipTest("descriptor limit too low to place a pipe above FD_SETSIZE")
            if soft < 1200:
                resource.setrlimit(resource.RLIMIT_NOFILE, (1200, hard))
                self.addCleanup(resource.setrlimit, resource.RLIMIT_NOFILE, (soft, hard))
            high_read, high_write = os.dup2(read_fd, 1100), os.dup2(write_fd, 1101)
            os.close(read_fd)
            os.close(write_fd)
            read_fd = write_fd = -1
            probe = subprocess.Popen([sys.executable, "-c", "pass"])
            probe.wait(timeout=30)
            setattr(probe, "_oxidex_ownership_read_fd", high_read)
            self.assertTrue(executor._ownership_probe_live(probe))
            os.write(high_write, b'{"phase": "exec", "ok": true}\n')
            setattr(probe, "_oxidex_lineage_status_fd", high_read)
            setattr(probe, "_oxidex_ownership_read_fd", -1)
            rows = executor._lineage_reports(probe, timeout=5, phase="exec")
            self.assertEqual(rows, [{"phase": "exec", "ok": True}])
            high_read = -1  # the reader closes it on EOF or leaves it attached
        finally:
            for descriptor in (read_fd, write_fd, high_write):
                if descriptor >= 0:
                    os.close(descriptor)
            if high_read >= 0:
                try:
                    os.close(high_read)
                except OSError:
                    pass

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "the lineage supervisor is Linux-only")
    def test_lineage_supervisor_falls_back_when_children_files_are_absent(self):
        """Kernels without /proc/<pid>/task/<tid>/children must still sweep and verify."""
        crippled = executor._LINUX_LINEAGE_SUPERVISOR.replace(
            'f"/proc/{me}/task/{task}/children"', 'f"/proc/{me}/task/{task}/children-absent"')
        self.assertNotEqual(crippled, executor._LINUX_LINEAGE_SUPERVISOR)
        with patch.object(executor, "_LINUX_LINEAGE_SUPERVISOR", crippled):
            exited = executor._run_record([sys.executable, "-c", "raise SystemExit(7)"],
                                          cwd=self.root, env=dict(os.environ), run=subprocess.run)
        self.assertEqual((exited["state"], exited["exit"]), ("exit_failed", 7), exited)

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "the lineage supervisor is Linux-only")
    def test_unconfirmed_supervised_start_does_not_strand_the_command(self):
        """A failed exec-report read must not leave an unpublished command running."""
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        command_pid = None
        original = executor._lineage_reports

        def fail_after_command_started(child, *, timeout, phase):
            nonlocal command_pid, write_fd
            if phase == "exec":
                os.close(write_fd)
                write_fd = -1
                command_pid = int(_read_reported_line(read_fd))
                raise ValueError("filedescriptor out of range in select()")
            return original(child, timeout=timeout, phase=phase)

        try:
            with patch.object(executor, "_lineage_reports", side_effect=fail_after_command_started):
                record = executor._run_record(
                    [sys.executable, "-c",
                     "import os, sys, time; os.write(int(sys.argv[1]), f'{os.getpid()}\\n'.encode()); "
                     "time.sleep(60)", str(write_fd)],
                    cwd=self.root, env=dict(os.environ), run=subprocess.run)
            self.assertEqual(record["state"], "spawn_failed", record)
            self.assertIsNotNone(command_pid)
            deadline = time.monotonic() + 10
            while executor._pid_live(command_pid) and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertFalse(executor._pid_live(command_pid), "the unpublished command was stranded")
            self.assertEqual(executor.unproven_children(), [])
        finally:
            for descriptor in (write_fd, read_fd):
                if descriptor >= 0:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
            if command_pid is not None:
                try:
                    os.kill(command_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    @unittest.skipUnless(sys.platform == "darwin", "Darwin kernel process identity")
    def test_darwin_zombie_descendant_identity_reports_exit_not_uncertainty(self):
        """A killed descendant awaiting its reaper is gone, not an unverifiable identity.

        proc_pidinfo has no BSD info for a zombie, and signal zero to one
        reparented to launchd is refused with EPERM; timeout cleanup then
        reported a spurious cleanup_error. The kernel's p_stat says SZOMB.
        """
        child = subprocess.Popen(["sleep", "30"])
        try:
            os.kill(child.pid, signal.SIGKILL)
            deadline = time.monotonic() + 10
            while (executor._darwin_kinfo([*executor._DARWIN_KERN_PROC, executor._DARWIN_KERN_PROC_PID, child.pid])
                   != [(child.pid, executor._DARWIN_SZOMB)]):
                if time.monotonic() >= deadline:
                    self.fail("child never became a zombie")
                time.sleep(0.01)
            with self.assertRaises(ProcessLookupError):
                executor._process_identity(child.pid)
        finally:
            child.wait(timeout=10)

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "the lineage supervisor is Linux-only")
    def test_sweep_request_before_supervisor_wait_still_sweeps(self):
        """A SIGUSR1 sweep request between the exec report and the wait loop must sweep.

        The supervisor is slowed right after it reports a successful exec, so
        the 0.2 s command timeout's sweep request lands in that interval. The
        command left the supervisor's session and closed its inherited
        descriptors, so only the supervisor's sweep can reach it.
        """
        slowed = re.sub(r'(\n    report\(phase="exec", ok=True[^\n]*\n)', r'\1    time.sleep(1.5)\n',
                        executor._LINUX_LINEAGE_SUPERVISOR, count=1)
        self.assertNotEqual(slowed, executor._LINUX_LINEAGE_SUPERVISOR)
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        command_pid = None
        program = (
            "import os, sys, time\n"
            "os.setsid()\n"
            "report = int(sys.argv[1])\n"
            "for name in os.listdir('/proc/self/fd'):\n"
            "    if name.isdigit() and int(name) > 2 and int(name) != report:\n"
            "        try:\n"
            "            os.close(int(name))\n"
            "        except OSError:\n"
            "            pass\n"
            "os.write(report, f'{os.getpid()}\\n'.encode())\n"
            "os.close(report)\n"
            "time.sleep(60)\n"
        )
        try:
            with patch.object(executor, "_LINUX_LINEAGE_SUPERVISOR", slowed), \
                 patch.object(executor, "COMMAND_TIMEOUT_SECONDS", 0.2):
                try:
                    executor._run_record([sys.executable, "-c", program, str(write_fd)],
                                         cwd=self.root, env=dict(os.environ), run=subprocess.run)
                except executor.OwnedChildCleanupIncomplete:
                    pass
            os.close(write_fd)
            write_fd = -1
            command_pid = int(_read_reported_line(read_fd))
            deadline = time.monotonic() + 10
            while executor._pid_live(command_pid) and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertFalse(executor._pid_live(command_pid),
                             "a sweep request before the wait loop left the command running")
        finally:
            for descriptor in (write_fd, read_fd):
                if descriptor >= 0:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
            if command_pid is not None:
                try:
                    os.kill(command_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "the lineage supervisor is Linux-only")
    def test_unverified_lineage_keeps_host_lock_held(self):
        """A supervisor killed after exec leaves its lineage unproven, and the lock held.

        The command left the session and closed every descriptor (so neither
        the process group nor the ownership probe can see it); only the
        supervisor's missing verdict says it may still run.
        """
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        command_pid = None
        program = (
            "import os, sys, time\n"
            "os.setsid()\n"
            "report = int(sys.argv[1])\n"
            "for name in os.listdir('/proc/self/fd'):\n"
            "    if name.isdigit() and int(name) != report:\n"
            "        try:\n"
            "            os.close(int(name))\n"
            "        except OSError:\n"
            "            pass\n"
            "os.write(report, f'{os.getpid()}\\n'.encode())\n"
            "os.close(report)\n"
            "time.sleep(60)\n"
        )

        def kill_supervisor_after_detach(pid, _pgid):
            nonlocal command_pid, write_fd
            os.close(write_fd)
            write_fd = -1
            command_pid = int(_read_reported_line(read_fd))
            os.kill(pid, signal.SIGKILL)

        try:
            with self.assertRaises(executor.LockRetained):
                with executor._HostLock(self.lock):
                    with self.assertRaises(executor.OwnedChildCleanupIncomplete):
                        executor._run_record([sys.executable, "-c", program, str(write_fd)],
                                             cwd=self.root, env=dict(os.environ), run=subprocess.run,
                                             started=kill_supervisor_after_detach)
            self.assertTrue(executor._pid_live(command_pid))
            self.assertEqual(_contend(self.lock), "blocked")
            marker = json.loads(executor._unproven_lineage_marker(self.lock.absolute()).read_text())
            hidden = [item.get("lineage_primary") or {} for item in marker["survivors"]]
            self.assertIn(command_pid, [item.get("pid") for item in hidden],
                          f"the marker must name the hidden command, not only its supervisor: {marker}")
            self.assertTrue(all(str(item.get("identity", "")).startswith("procfs-start:")
                                for item in hidden if item.get("pid") == command_pid))
        finally:
            for descriptor in (write_fd, read_fd):
                if descriptor >= 0:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
            if command_pid is not None:
                try:
                    os.kill(command_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for stream in list(executor._RETAINED_LOCKS):
                executor._RETAINED_LOCKS.remove(stream)
                stream.close()  # the in-process owner exits; closing frees the description

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "the lineage supervisor is Linux-only")
    def test_inherited_ignored_sigchld_does_not_break_supervision(self):
        """A launcher that ignores SIGCHLD must not make ordinary stages unverifiable."""
        previous = signal.signal(signal.SIGCHLD, signal.SIG_IGN)
        try:
            record = executor._run_record(["/bin/true"], cwd=self.root, env=dict(os.environ),
                                          run=subprocess.run)
        finally:
            signal.signal(signal.SIGCHLD, previous)
        self.assertEqual((record["state"], record["exit"]), ("ok", 0), record)

    def test_unverified_lineage_retention_survives_the_owner_process(self):
        """A lineage nobody can see must keep the host lock refused after its owner exits.

        When the only evidence of a possibly live command is the supervisor's
        missing verdict, no process holds the lock's description, so parking
        the stream in-process ends with the process. A durable marker next to
        the lock must keep every later owner (and standalone recovery) out.
        """
        child = executor._spawn([sys.executable, "-c", "pass"], stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
        child.wait(timeout=30)
        verdict = "exited; its lineage supervisor never proved every descendant gone"
        with patch.object(executor, "_lineage_unverified", return_value=verdict):
            with self.assertRaises(executor.LockRetained):
                with executor._HostLock(self.lock):
                    pass
        # The owner process ends: its parked stream closes and the flock is free.
        for stream in list(executor._RETAINED_LOCKS):
            executor._RETAINED_LOCKS.remove(stream)
            stream.close()
        self.assertEqual(_contend(self.lock), "acquired")
        with self.assertRaisesRegex(executor.Refused, "unproven"):
            with executor._HostLock(self.lock):
                pass
        lease = qualification.TransitionLease(
            lease=self.lock, run_id="after-unverified", owner_receipt=self.root / "owner.json",
            heartbeat_receipt=self.root / "heartbeat.jsonl", expiry_receipt=self.root / "expiry.json",
            release_receipt=self.root / "release.json")
        with self.assertRaisesRegex(executor.Refused, "unproven"):
            with lease:
                pass

    @unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0, "file modes do not bind root")
    def test_unverified_lineage_refusal_survives_a_failed_marker_write(self):
        """If the marker cannot be written, the refusal must still outlive the owner."""
        child = executor._spawn([sys.executable, "-c", "pass"], stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
        child.wait(timeout=30)
        self.lock.touch()
        self.addCleanup(os.chmod, self.lock, 0o644)
        verdict = "exited; its lineage supervisor never proved every descendant gone"
        with patch.object(executor, "_lineage_unverified", return_value=verdict), \
             patch.object(executor, "_atomic", side_effect=OSError(28, "No space left on device")):
            with self.assertRaises(executor.LockRetained):
                with executor._HostLock(self.lock):
                    pass
        for stream in list(executor._RETAINED_LOCKS):
            executor._RETAINED_LOCKS.remove(stream)
            stream.close()
        with self.assertRaises(OSError):
            with executor._HostLock(self.lock):
                pass

    @_without_lineage_supervisor
    def test_unreleased_inherited_ownership_keeps_host_lock_held(self):
        """Incomplete ownership release must reach the lock owner's release decision.

        A detached descendant in its own session keeps the inherited lock and
        ownership descriptors after the direct child exits 0. The command is
        refused as incomplete, and the lock owner must then retain the lock:
        an explicit unlock would also release it for that live descendant.
        """
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        descendant_pid = None
        descendant_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import subprocess, sys\n"
            "subprocess.Popen([sys.executable, '-c', sys.argv[2], sys.argv[1]], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
        )
        try:
            with self.assertRaises(executor.LockRetained) as retained:
                with executor._HostLock(self.lock):
                    with self.assertRaises(executor.OwnedChildCleanupIncomplete):
                        executor._run_record(
                            [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                            cwd=self.root, env=dict(os.environ), run=subprocess.run,
                        )
                    os.close(write_fd)
                    write_fd = -1
                    descendant_pid = int(_read_reported_line(read_fd))
            self.assertTrue(executor._pid_live(descendant_pid))
            self.assertIn("inherited", executor.describe_survivors(retained.exception.survivors))
            self.assertEqual(_contend(self.lock), "blocked")
            os.kill(descendant_pid, signal.SIGKILL)
            deadline = time.monotonic() + 10
            while executor.release_retained_locks():
                if time.monotonic() >= deadline:
                    self.fail("retained lock stayed held after the inheriting descendant exited")
                time.sleep(0.05)
            self.assertEqual(_contend(self.lock), "acquired")
        finally:
            for descriptor in (write_fd, read_fd):
                if descriptor >= 0:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
            if descendant_pid is not None:
                try:
                    os.kill(descendant_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def config(self, *, write=True, tests=True):
        commands = {stage: {"argv": [stage]} for stage in ("generate", "build", "read")}
        if tests:
            commands["test"] = {"argv": ["test"]}
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
        if stage == "test":
            body["test_suite"] = {
                "commands": [{"argv": ["cargo", "test"], "exit": 0, "duration_seconds": 0.5,
                              "passed": 3, "failed": 0, "ignored": 1, "measured": 0,
                              "filtered_out": 0, "targets": 2}],
                "totals": {"passed": 3, "failed": 0, "ignored": 1, "measured": 0,
                           "filtered_out": 0, "targets": 2},
                "log": body["raw_report"], "target_directory": env["CARGO_TARGET_DIR"] + "/test-suite",
                "exiftool_oracle": {"version": env["OXIDEX_REHEARSAL_RELEASE"], "docx_filetype": "DOCX",
                                    "perl_modules_available": True,
                                    "tree_realpath": body["native_identity"]["source"]["path"],
                                    "lib": {"exiftool_pm_sha256": body["native_identity"]["lib"]["exiftool_pm_sha256"]},
                                    "perl": body["native_identity"]["perl"]},
                "fixture_corpus": {"manifest": {"sha256": "8" * 64, "file_count": 4249},
                                   "verified_before_run": True, "verified_after_run": True},
            }
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

    def test_release_tests_run_after_build_and_before_read(self):
        self.initialize(self.config())
        journal = self.execute()
        self.assertEqual(journal["phase"], "complete")
        self.assertEqual(journal["scope"]["release_tests"], "passed_per_release")
        per_release = [argv[0] for argv, _, _ in self.calls]
        self.assertEqual(per_release, ["generate", "build", "test", "read", "write"] * len(self.releases))
        for release in self.releases:
            self.assertEqual(journal["releases"][release]["stages"]["test"], "passed")
            self.assertEqual(journal["releases"][release]["reports"]["test"]["denominator"], 3)

    def test_failed_release_tests_stop_the_release_before_read(self):
        self.initialize(self.config())
        original = self.command
        def failing_tests(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "test":
                return subprocess.CompletedProcess(argv, 2, "tests failed", "")
            return result
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            journal = executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                       run=failing_tests, checkout=self.checkout)
        self.assertEqual(journal["phase"], "failed")
        release = next(name for name, row in journal["releases"].items() if row["failure"])
        self.assertEqual(journal["releases"][release]["failure"]["stage"], "test")
        self.assertNotIn("read", [argv[0] for argv, _, _ in self.calls])

    def test_test_report_with_failures_cannot_pass_even_after_exit_zero(self):
        self.initialize(self.config())
        original = self.command
        def forged(argv, **kwargs):
            result = original(argv, **kwargs)
            if argv[0] == "test":
                report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                body = json.loads(report.read_text())
                body["test_suite"]["totals"]["failed"] = 1
                report.write_text(json.dumps(body))
            return result
        with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            journal = executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                       run=forged, checkout=self.checkout)
        self.assertEqual(journal["phase"], "failed")
        self.assertNotIn("read", [argv[0] for argv, _, _ in self.calls])

    def test_test_report_graded_by_another_exiftool_cannot_pass(self):
        for field, value in (("version", "13.55"), ("docx_filetype", "ZIP"), ("perl_modules_available", False),
                             ("fixture_corpus", None)):
            with self.subTest(field=field):
                self.run_dir = self.root / f"foreign-oracle-{field}"
                self.calls.clear()
                self.initialize(self.config())
                original = self.command
                def foreign(argv, **kwargs):
                    result = original(argv, **kwargs)
                    if argv[0] == "test":
                        report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                        body = json.loads(report.read_text())
                        if field == "fixture_corpus":
                            body["test_suite"]["fixture_corpus"]["verified_after_run"] = False
                        else:
                            body["test_suite"]["exiftool_oracle"][field] = value
                        report.write_text(json.dumps(body))
                    return result
                with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
                    journal = executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                               run=foreign, checkout=self.checkout)
                self.assertEqual(journal["phase"], "failed")
                self.assertNotIn("read", [argv[0] for argv, _, _ in self.calls])

    def test_test_suite_totals_must_be_the_sum_of_counted_commands(self):
        native = {"release": "13.59", "perl": {"path": "/perl", "sha256": "c" * 64},
                  "source": {"path": "/exiftool"}, "lib": {"path": "/exiftool/lib", "exiftool_pm_sha256": "d" * 64}}
        base = {"release": "13.59", "denominator": 1, "raw_report": {"path": "raw", "sha256": "a" * 64},
                "native_identity": native,
                "test_suite": {"log": {"path": "raw", "sha256": "a" * 64},
                               "exiftool_oracle": {"version": "13.59", "docx_filetype": "DOCX",
                                                   "perl_modules_available": True, "tree_realpath": "/exiftool",
                                                   "lib": {"exiftool_pm_sha256": "d" * 64},
                                                   "perl": {"path": "/perl", "sha256": "c" * 64}},
                               "fixture_corpus": {"manifest": {"sha256": "8" * 64, "file_count": 1},
                                                  "verified_before_run": True, "verified_after_run": True},
                               "totals": {"passed": 1, "failed": 0, "ignored": 0, "measured": 0,
                                          "filtered_out": 0, "targets": 1}}}
        counted = {"exit": 0, "passed": 1, "failed": 0, "ignored": 0, "measured": 0, "filtered_out": 0, "targets": 1}
        good = json.loads(json.dumps(base)); good["test_suite"]["commands"] = [dict(counted)]
        executor._require_test_suite_proof(good)
        for label, commands in (("uncounted command", [{"exit": 0}]),
                                ("totals exceed commands", [dict(counted, passed=0, targets=1)]),
                                ("string count", [dict(counted, passed="1")]),
                                ("second command uncounted", [dict(counted), {"exit": 0}])):
            with self.subTest(label=label):
                forged = json.loads(json.dumps(base)); forged["test_suite"]["commands"] = commands
                with self.assertRaisesRegex(executor.Refused, "counted zero-failure"):
                    executor._require_test_suite_proof(forged)

    def test_test_oracle_must_be_the_validated_native_identity(self):
        """A custom wrapper cannot self-report an oracle other than the selected native tree."""
        for label, mutate in (
                ("foreign tree", lambda oracle, native: oracle.update(tree_realpath="/opt/homebrew/exiftool")),
                ("foreign library", lambda oracle, native: oracle["lib"].update(exiftool_pm_sha256="0" * 64)),
                ("foreign perl", lambda oracle, native: oracle.update(perl={"path": "/opt/homebrew/bin/perl",
                                                                           "sha256": "0" * 64})),
                ("identity missing", lambda oracle, native: oracle.pop("perl"))):
            with self.subTest(label=label):
                self.run_dir = self.root / f"oracle-{label.replace(' ', '-')}"
                self.calls.clear()
                self.initialize(self.config())
                original = self.command
                def forged(argv, **kwargs):
                    result = original(argv, **kwargs)
                    if argv[0] == "test":
                        report = Path(kwargs["env"]["OXIDEX_REHEARSAL_REPORT"])
                        body = json.loads(report.read_text())
                        mutate(body["test_suite"]["exiftool_oracle"], body["native_identity"])
                        report.write_text(json.dumps(body))
                    return result
                with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
                    journal = executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                               run=forged, checkout=self.checkout)
                self.assertEqual(journal["phase"], "failed")
                self.assertNotIn("read", [argv[0] for argv, _, _ in self.calls])
        # The honest wrapper, whose oracle matches the native identity, still passes.
        self.run_dir = self.root / "oracle-honest"
        self.initialize(self.config())
        self.assertEqual(self.execute()["phase"], "complete")

    def test_absent_test_command_is_visible_as_unsupported(self):
        self.initialize(self.config(tests=False))
        journal = self.execute()
        self.assertEqual(journal["scope"]["release_tests"], "unsupported_for_one_or_more_releases")
        self.assertTrue(all(row["stages"]["test"] == "unsupported" for row in journal["releases"].values()))
        self.assertNotIn("test", [argv[0] for argv, _, _ in self.calls])

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

    @_without_lineage_supervisor
    def test_native_success_refuses_detached_descendant_retaining_ownership(self):
        """Native success retains active state when detached owned work keeps the proof."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        children: list[subprocess.Popen[str]] = []
        descendant_pid = None
        original_popen = executor.subprocess.Popen
        descendant_program = (
            "import os, sys, time\n"
            "os.write(int(sys.argv[1]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import subprocess, sys\n"
            "subprocess.Popen([sys.executable, '-c', sys.argv[2], sys.argv[1]], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
        )

        def capture_child(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            children.append(child)
            return child

        def successful_probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=60,
            )
            return ready_probe(release)

        try:
            failure = None
            with patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=successful_probe), \
                 patch.object(executor.subprocess, "Popen", side_effect=capture_child):
                try:
                    executor._run_native(
                        self.run_dir, journal, release, docs, config,
                        self.cache, self.sources, subprocess.run,
                    )
                except BaseException as exc:
                    failure = exc
            descendant_pid = int(_read_reported_line(read_fd))
            self.assertIsInstance(failure, executor.OwnedChildCleanupIncomplete)
            self.assertTrue(executor._pid_live(descendant_pid))
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "running")
            self.assertIsNotNone(persisted["active"])
        finally:
            for descriptor in (write_fd, read_fd):
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            if descendant_pid is not None:
                try:
                    os.kill(descendant_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for child in children:
                executor._close_ownership_probe(child)

    def test_native_child_record_failure_reaps_child_before_clearing_active(self):
        """A failed post-spawn journal write cannot publish failure with a live child."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        process_ids: list[int] = []
        failed = False
        original_store = executor._store_journal

        def probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=60,
            )
            self.fail("native child unexpectedly completed")

        def fail_first_child_record(run_dir, value):
            nonlocal failed
            active = value.get("active")
            child = active.get("child") if isinstance(active, dict) else None
            if isinstance(child, dict) and not failed:
                failed = True
                process_ids.append(child["pid"])
                raise OSError("native child journal persistence failed")
            return original_store(run_dir, value)

        try:
            with patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=probe), \
                 patch.object(executor, "_store_journal", side_effect=fail_first_child_record):
                result = executor._run_native(
                    self.run_dir, journal, release, docs, config,
                    self.cache, self.sources, subprocess.run,
                )
            self.assertIsNone(result)
            self.assertEqual(len(process_ids), 1)
            self.assertFalse(executor._pid_live(process_ids[0]))
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "failed")
            self.assertIsNone(persisted["active"])
        finally:
            for pid in process_ids:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    os.waitpid(pid, 0)
                except ChildProcessError:
                    pass

    def test_native_child_record_failure_preserves_active_when_cleanup_is_incomplete(self):
        """A post-spawn journal fault cannot clear active when cleanup is unverified."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        process_ids: list[int] = []
        children: list[subprocess.Popen[str]] = []
        failed = False
        original_store = executor._store_journal
        original_popen = executor.subprocess.Popen

        def capture_child(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not children:
                children.append(child)
            return child

        def probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=60,
            )
            self.fail("native child unexpectedly completed")

        def fail_first_child_record(run_dir, value):
            nonlocal failed
            active = value.get("active")
            child = active.get("child") if isinstance(active, dict) else None
            if isinstance(child, dict) and not failed:
                failed = True
                process_ids.append(child["pid"])
                raise OSError("native child journal persistence failed")
            return original_store(run_dir, value)

        try:
            with patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=probe), \
                 patch.object(executor, "_store_journal", side_effect=fail_first_child_record), \
                 patch.object(executor.subprocess, "Popen", side_effect=capture_child), \
                 patch.object(executor, "_bounded_timeout_cleanup",
                              side_effect=OSError("bounded cleanup failed")), \
                 patch.object(executor, "_emergency_reap_group",
                              side_effect=OSError("emergency cleanup failed")), \
                 self.assertRaises(executor.OwnedChildCleanupIncomplete):
                executor._run_native(
                    self.run_dir, journal, release, docs, config,
                    self.cache, self.sources, subprocess.run,
                )
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "running")
            self.assertIsNotNone(persisted["active"])
        finally:
            for pid in process_ids:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for child in children:
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                for stream in (child.stdout, child.stderr):
                    if stream is not None:
                        stream.close()
                executor._close_ownership_probe(child)

    def test_native_child_record_interrupt_preserves_interrupt_and_active_when_cleanup_is_incomplete(self):
        """Incomplete cleanup annotates the real persistence interrupt without replacing it."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        process_ids: list[int] = []
        children: list[subprocess.Popen[str]] = []
        interrupted = False
        original_store = executor._store_journal
        original_popen = executor.subprocess.Popen

        def capture_child(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not children:
                children.append(child)
            return child

        def probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=60,
            )
            self.fail("native child unexpectedly completed")

        def interrupt_first_child_record(run_dir, value):
            nonlocal interrupted
            active = value.get("active")
            child = active.get("child") if isinstance(active, dict) else None
            if isinstance(child, dict) and not interrupted:
                interrupted = True
                process_ids.append(child["pid"])
                original_store(run_dir, value)
                raise KeyboardInterrupt("native child journal persistence interrupted")
            return original_store(run_dir, value)

        failure = None
        try:
            with patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=probe), \
                 patch.object(executor, "_store_journal", side_effect=interrupt_first_child_record), \
                 patch.object(executor.subprocess, "Popen", side_effect=capture_child), \
                 patch.object(executor, "_bounded_timeout_cleanup",
                              side_effect=OSError("bounded cleanup failed")), \
                 patch.object(executor, "_emergency_reap_group",
                              side_effect=OSError("emergency cleanup failed")):
                try:
                    executor._run_native(
                        self.run_dir, journal, release, docs, config,
                        self.cache, self.sources, subprocess.run,
                    )
                except BaseException as exc:
                    failure = exc
            self.assertIsInstance(failure, KeyboardInterrupt)
            self.assertEqual(str(failure), "native child journal persistence interrupted")
            self.assertEqual(
                getattr(failure, "_oxidex_owned_child_cleanup", None),
                "incomplete",
            )
            notes = list(getattr(failure, "__notes__", []))
            self.assertTrue(any("bounded cleanup failed" in note for note in notes))
            self.assertTrue(any("emergency cleanup failed" in note for note in notes))
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "running")
            self.assertIsNotNone(persisted["active"])
            self.assertEqual(len(process_ids), 1)
            self.assertTrue(executor._pid_live(process_ids[0]))

            arguments = [
                "--matrix", "matrix", "--repository", "repository", "--output", "output",
                "--target-root", "target", "--lease", "lease", "--run-id", "run",
                "--owner-receipt", "owner", "--heartbeat-receipt", "heartbeat",
                "--expiry-receipt", "expiry", "--release-receipt", "release",
                "--handoff-receipt", "handoff",
            ]
            stderr = io.StringIO()
            with patch.object(qualification, "run_qualification", side_effect=failure), \
                 redirect_stderr(stderr):
                self.assertEqual(qualification.main(arguments), 130)
            rendered = stderr.getvalue()
            self.assertIn("durable recovery incomplete or unverified", rendered)
            self.assertIn("recovery detail: bounded owned-child cleanup failed", rendered)
            self.assertIn("recovery detail: emergency owned-child cleanup failed", rendered)
        finally:
            for pid in process_ids:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for child in children:
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                for stream in (child.stdout, child.stderr):
                    if stream is not None:
                        stream.close()
                executor._close_ownership_probe(child)

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
                    with self.subTest(process=name):
                        self.assertFalse(executor._pid_live(process_ids[name]))
                recovered = executor.recover(
                    self.run_dir, self.cache, self.sources, host_lock_fd=held.capability,
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

    def test_native_creation_window_interrupt_reaps_created_child(self):
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen

        def interrupt_after_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
                os.kill(os.getpid(), signal.SIGINT)
            return child

        def interrupted_probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=60,
            )
            self.fail("native creation-window child unexpectedly returned")

        try:
            with executor._HostLock(self.lock) as held, \
                 patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=interrupted_probe), \
                 patch.object(executor.subprocess, "Popen", side_effect=interrupt_after_creation):
                with self.assertRaises(KeyboardInterrupt):
                    executor._run_native(
                        self.run_dir, journal, release, docs, config,
                        self.cache, self.sources, subprocess.run,
                    )
                self.assertEqual(len(created), 1)
                self.assertFalse(executor._pid_live(created[0].pid))
                recovered = executor.recover(
                    self.run_dir, self.cache, self.sources, host_lock_fd=held.capability,
                )
                self.assertEqual(recovered["phase"], "interrupted")
        finally:
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

    def test_standard_creation_window_interrupt_reaps_created_child(self):
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen

        def interrupt_after_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
                os.kill(os.getpid(), signal.SIGINT)
            return child

        try:
            with patch.object(executor.subprocess, "Popen", side_effect=interrupt_after_creation):
                with self.assertRaises(KeyboardInterrupt):
                    executor._run_record(
                        [sys.executable, "-c", "import time; time.sleep(60)"],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run,
                    )
            self.assertEqual(len(created), 1)
            self.assertFalse(executor._pid_live(created[0].pid))
        finally:
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass

    def test_creation_window_sigint_preserves_ignored_disposition(self):
        owner: list[subprocess.Popen[str] | None] = [None]
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen
        previous_handler = signal.getsignal(signal.SIGINT)
        failure = None

        def interrupt_after_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
                os.kill(os.getpid(), signal.SIGINT)
            return child

        try:
            signal.signal(signal.SIGINT, signal.SIG_IGN)
            with patch.object(executor.subprocess, "Popen", side_effect=interrupt_after_creation):
                try:
                    child = executor._spawn_with_deferred_sigint(
                        owner,
                        [sys.executable, "-c", "import time; time.sleep(60)"],
                        text=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL, start_new_session=True, close_fds=False,
                    )
                except BaseException as exc:
                    failure = exc
                    child = None
            self.assertIsNone(failure)
            self.assertIs(child, owner[0])
        finally:
            signal.signal(signal.SIGINT, previous_handler)
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=5)
                executor._close_ownership_probe(child)

    def test_creation_window_sigint_invokes_prior_custom_handler_after_publication(self):
        owner: list[subprocess.Popen[str] | None] = [None]
        created: list[subprocess.Popen[str]] = []
        calls: list[tuple[int, bool, bool]] = []
        original_popen = executor.subprocess.Popen
        previous_handler = signal.getsignal(signal.SIGINT)
        failure = None

        def custom_handler(signum, frame):
            calls.append((signum, frame is not None, owner[0] is not None))

        def interrupt_after_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            created.append(child)
            os.kill(os.getpid(), signal.SIGINT)
            return child

        try:
            signal.signal(signal.SIGINT, custom_handler)
            with patch.object(executor.subprocess, "Popen", side_effect=interrupt_after_creation):
                try:
                    child = executor._spawn_with_deferred_sigint(
                        owner,
                        [sys.executable, "-c", "import time; time.sleep(60)"],
                        text=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL, start_new_session=True, close_fds=False,
                    )
                except BaseException as exc:
                    failure = exc
                    child = None
            self.assertIsNone(failure)
            self.assertIs(child, owner[0])
            self.assertEqual(calls, [(signal.SIGINT, True, True)])
        finally:
            signal.signal(signal.SIGINT, previous_handler)
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=5)
                executor._close_ownership_probe(child)

    def test_creation_window_sigint_cleans_child_before_custom_system_exit(self):
        """A non-KeyboardInterrupt handler outcome cannot strand the published child."""
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen
        previous_handler = signal.getsignal(signal.SIGINT)

        def custom_handler(_signum, _frame):
            raise SystemExit("custom SIGINT exit")

        def interrupt_after_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
                os.kill(os.getpid(), signal.SIGINT)
            return child

        try:
            signal.signal(signal.SIGINT, custom_handler)
            with patch.object(executor.subprocess, "Popen", side_effect=interrupt_after_creation), \
                 self.assertRaisesRegex(SystemExit, "custom SIGINT exit"):
                executor._run_record(
                    [sys.executable, "-c", "import time; time.sleep(60)"],
                    cwd=self.root, env=dict(os.environ), run=subprocess.run,
                )
            self.assertEqual(len(created), 1)
            self.assertFalse(executor._pid_live(created[0].pid))
        finally:
            signal.signal(signal.SIGINT, previous_handler)
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

    def test_failed_spawn_replays_deferred_sigint_to_default_handler(self):
        owner: list[subprocess.Popen[str] | None] = [None]

        def fail_after_sigint(*_args, **_kwargs):
            os.kill(os.getpid(), signal.SIGINT)
            raise FileNotFoundError("missing executable")

        failure = None
        with patch.object(executor.subprocess, "Popen", side_effect=fail_after_sigint):
            try:
                executor._spawn_with_deferred_sigint(
                    owner, ["missing"], text=True, stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    start_new_session=True, close_fds=False,
                )
            except BaseException as exc:
                failure = exc
        self.assertIsInstance(failure, KeyboardInterrupt)
        self.assertIsNone(owner[0])

    def test_failed_spawn_ignored_deferred_sigint_preserves_spawn_failure(self):
        owner: list[subprocess.Popen[str] | None] = [None]
        previous_handler = signal.getsignal(signal.SIGINT)

        def fail_after_sigint(*_args, **_kwargs):
            os.kill(os.getpid(), signal.SIGINT)
            raise FileNotFoundError("missing executable")

        try:
            signal.signal(signal.SIGINT, signal.SIG_IGN)
            with patch.object(executor.subprocess, "Popen", side_effect=fail_after_sigint), \
                 self.assertRaisesRegex(FileNotFoundError, "missing executable"):
                executor._spawn_with_deferred_sigint(
                    owner, ["missing"], text=True, stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    start_new_session=True, close_fds=False,
                )
        finally:
            signal.signal(signal.SIGINT, previous_handler)
        self.assertIsNone(owner[0])

    def test_failed_spawn_invokes_returning_custom_handler_before_spawn_failure(self):
        owner: list[subprocess.Popen[str] | None] = [None]
        previous_handler = signal.getsignal(signal.SIGINT)
        calls: list[tuple[int, bool, bool]] = []

        def custom_handler(signum, frame):
            calls.append((signum, frame is not None, owner[0] is None))

        def fail_after_sigint(*_args, **_kwargs):
            os.kill(os.getpid(), signal.SIGINT)
            raise FileNotFoundError("missing executable")

        try:
            signal.signal(signal.SIGINT, custom_handler)
            with patch.object(executor.subprocess, "Popen", side_effect=fail_after_sigint), \
                 self.assertRaisesRegex(FileNotFoundError, "missing executable"):
                executor._spawn_with_deferred_sigint(
                    owner, ["missing"], text=True, stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    start_new_session=True, close_fds=False,
                )
        finally:
            signal.signal(signal.SIGINT, previous_handler)
        self.assertEqual(calls, [(signal.SIGINT, True, True)])

    def test_failed_spawn_propagates_raising_custom_handler_outcome(self):
        owner: list[subprocess.Popen[str] | None] = [None]
        previous_handler = signal.getsignal(signal.SIGINT)

        def custom_handler(_signum, _frame):
            raise SystemExit("custom SIGINT exit")

        def fail_after_sigint(*_args, **_kwargs):
            os.kill(os.getpid(), signal.SIGINT)
            raise FileNotFoundError("missing executable")

        failure = None
        try:
            signal.signal(signal.SIGINT, custom_handler)
            with patch.object(executor.subprocess, "Popen", side_effect=fail_after_sigint):
                try:
                    executor._spawn_with_deferred_sigint(
                        owner, ["missing"], text=True, stdin=subprocess.DEVNULL,
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                        start_new_session=True, close_fds=False,
                    )
                except BaseException as exc:
                    failure = exc
        finally:
            signal.signal(signal.SIGINT, previous_handler)
        self.assertIsInstance(failure, SystemExit)
        self.assertEqual(str(failure), "custom SIGINT exit")
        self.assertIsNone(owner[0])

    def test_failed_spawn_without_deferred_sigint_preserves_spawn_failure(self):
        owner: list[subprocess.Popen[str] | None] = [None]
        missing = FileNotFoundError("missing executable")

        with patch.object(executor.subprocess, "Popen", side_effect=missing), \
             self.assertRaisesRegex(FileNotFoundError, "missing executable") as caught:
            executor._spawn_with_deferred_sigint(
                owner, ["missing"], text=True, stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                start_new_session=True, close_fds=False,
            )
        self.assertIs(caught.exception, missing)
        self.assertIsNone(owner[0])

    def test_native_creation_defers_process_sigint_delivered_with_cadence_thread(self):
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen

        def interrupt_from_unblocked_thread_after_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
                sender = threading.Thread(target=os.kill, args=(os.getpid(), signal.SIGINT))
                sender.start()
                sender.join(timeout=5)
                self.assertFalse(sender.is_alive())
            return child

        def interrupted_probe(*_args, run, **_kwargs):
            run([sys.executable, "-c", "import time; time.sleep(60)"], cwd=str(self.root),
                env=dict(os.environ), text=True, capture_output=True, timeout=60)

        try:
            with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=interrupted_probe), \
                 patch.object(executor.subprocess, "Popen", side_effect=interrupt_from_unblocked_thread_after_creation):
                with self.assertRaises(KeyboardInterrupt):
                    executor._run_native(self.run_dir, journal, release, docs, config,
                                         self.cache, self.sources, subprocess.run)
            self.assertEqual(len(created), 1)
            self.assertFalse(executor._pid_live(created[0].pid))
        finally:
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass

    def test_native_timeout_cleanup_interrupt_reaps_created_child(self):
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen
        original_cleanup = executor._bounded_timeout_cleanup
        cleanup_calls = 0

        def track_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
            return child

        def interrupt_first_cleanup(child):
            nonlocal cleanup_calls
            cleanup_calls += 1
            if cleanup_calls == 1:
                raise KeyboardInterrupt("interrupt during native timeout cleanup")
            return original_cleanup(child)

        def interrupted_probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=0.01,
            )
            self.fail("native timeout child unexpectedly returned")

        try:
            with executor._HostLock(self.lock) as held, \
                 patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=interrupted_probe), \
                 patch.object(executor.subprocess, "Popen", side_effect=track_creation), \
                 patch.object(executor, "_bounded_timeout_cleanup",
                              side_effect=interrupt_first_cleanup):
                with self.assertRaisesRegex(KeyboardInterrupt, "native timeout cleanup"):
                    executor._run_native(
                        self.run_dir, journal, release, docs, config,
                        self.cache, self.sources, subprocess.run,
                    )
                self.assertEqual(cleanup_calls, 2)
                self.assertEqual(len(created), 1)
                self.assertFalse(executor._pid_live(created[0].pid))
                recovered = executor.recover(
                    self.run_dir, self.cache, self.sources, host_lock_fd=held.capability,
                )
                self.assertEqual(recovered["phase"], "interrupted")
        finally:
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass

    def test_native_timeout_cleanup_failure_preserves_active_journal(self):
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen

        def track_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            if not created:
                created.append(child)
            return child

        def timed_out_probe(*_args, run, **_kwargs):
            run([sys.executable, "-c", "import time; time.sleep(60)"], cwd=str(self.root),
                env=dict(os.environ), text=True, capture_output=True, timeout=0.01)

        try:
            with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=timed_out_probe), \
                 patch.object(executor.subprocess, "Popen", side_effect=track_creation), \
                 patch.object(executor, "_bounded_timeout_cleanup", side_effect=OSError("bounded failed")), \
                 patch.object(executor, "_emergency_reap_group", side_effect=OSError("emergency failed")), \
                 patch.object(executor, "_group_live", return_value=True):
                with self.assertRaisesRegex(OSError, "cleanup remains incomplete"):
                    executor._run_native(self.run_dir, journal, release, docs, config,
                                         self.cache, self.sources, subprocess.run)
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "running")
            self.assertEqual(persisted["releases"][release]["stages"]["native"], "running")
            self.assertIsInstance(persisted["active"]["child"]["pid"], int)
        finally:
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.communicate(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

    def test_native_oracle_record_cannot_hide_incomplete_cleanup(self):
        """The native oracle folds runner OSErrors into command records.

        OwnedChildCleanupIncomplete is an OSError, so the oracle's own
        ``_run`` turns it into an ordinary ``spawn_failed`` row. The native
        stage must still keep its active child and refuse a terminal state.
        """
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen
        rows: list[dict] = []

        def track_creation(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            created.append(child)
            return child

        def oracle_probe(*_args, run, **_kwargs):
            # The real oracle runner: it catches OSError into a record.
            rows.append(executor.native_oracle._run(
                [sys.executable, "-c", "import time; time.sleep(60)"], run))
            return {"state": "refused", "cases": [], "commands": rows}

        try:
            with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=oracle_probe), \
                 patch.object(executor.native_oracle, "TIMEOUT", 0.05), \
                 patch.object(executor.subprocess, "Popen", side_effect=track_creation), \
                 patch.object(executor, "_bounded_timeout_cleanup", side_effect=OSError("bounded failed")), \
                 patch.object(executor, "_emergency_reap_group", side_effect=OSError("emergency failed")), \
                 patch.object(executor, "_group_live", return_value=True):
                with self.assertRaisesRegex(executor.OwnedChildCleanupIncomplete, "cleanup remains incomplete"):
                    executor._run_native(self.run_dir, journal, release, docs, config,
                                         self.cache, self.sources, subprocess.run)
            self.assertEqual(rows[0]["state"], "spawn_failed")
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "running")
            self.assertEqual(persisted["releases"][release]["stages"]["native"], "running")
            self.assertIsInstance(persisted["active"]["child"]["pid"], int)
        finally:
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.communicate(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

    def test_native_probe_stops_spawning_after_incomplete_cleanup(self):
        """Later oracle commands must not run or overwrite the surviving child's identity."""
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        created: list[subprocess.Popen[str]] = []
        original_popen = executor.subprocess.Popen
        rows: list[dict] = []

        def track_creation(argv, *args, **kwargs):
            child = original_popen(argv, *args, **kwargs)
            if any("time.sleep(60)" in str(part) for part in argv):
                created.append(child)
            return child

        def two_command_probe(*_args, run, **_kwargs):
            for _ in range(2):
                rows.append(executor.native_oracle._run(
                    [sys.executable, "-c", "import time; time.sleep(60)"], run))
            return {"state": "refused", "cases": [], "commands": rows}

        try:
            with patch.object(executor.native_oracle, "probe_materialized_native", side_effect=two_command_probe), \
                 patch.object(executor.native_oracle, "TIMEOUT", 0.05), \
                 patch.object(executor.subprocess, "Popen", side_effect=track_creation), \
                 patch.object(executor, "_bounded_timeout_cleanup", side_effect=OSError("bounded failed")), \
                 patch.object(executor, "_emergency_reap_group", side_effect=OSError("emergency failed")), \
                 patch.object(executor, "_group_live", return_value=True):
                with self.assertRaises(executor.OwnedChildCleanupIncomplete):
                    executor._run_native(self.run_dir, journal, release, docs, config,
                                         self.cache, self.sources, subprocess.run)
            self.assertEqual(len(created), 1, "a second oracle command ran after incomplete cleanup")
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["active"]["child"]["pid"], created[0].pid)
        finally:
            for child in created:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.communicate(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

    def test_checkout_head_verification_cannot_hide_incomplete_cleanup(self):
        """A HEAD probe whose cleanup is unverified is not an ordinary refusal."""
        with patch.object(executor, "_tracked_run",
                          side_effect=executor.OwnedChildCleanupIncomplete("cleanup remains incomplete")):
            with self.assertRaises(executor.OwnedChildCleanupIncomplete):
                executor._verify_checkout_head(self.root, "0" * 40, subprocess.run)

    def test_native_timeout_marker_bypass_preserves_active_journal(self):
        self.initialize(self.config())
        journal, docs, config = executor._load_journal(self.run_dir, self.cache, self.sources)
        release = self.releases[0]
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        descendant_program = (
            "import os, signal, sys, time\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "while os.getppid() == int(sys.argv[1]):\n"
            "    time.sleep(0.001)\n"
            "os.write(int(sys.argv[2]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[2]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import os, signal, subprocess, sys\n"
            "ready_fd = int(sys.argv[1])\n"
            "def terminate(_signum, _frame):\n"
            "    fd_root = '/dev/fd' if os.path.isdir('/dev/fd') else '/proc/self/fd'\n"
            "    for name in os.listdir(fd_root):\n"
            "        if name.isdigit() and int(name) > 2 and int(name) != ready_fd:\n"
            "            try:\n"
            "                os.close(int(name))\n"
            "            except OSError:\n"
            "                pass\n"
            "    descendant = subprocess.Popen([sys.executable, '-c', sys.argv[2], "
            "str(os.getpid()), str(ready_fd)], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
            "    os.close(ready_fd)\n"
            "    raise SystemExit(0)\n"
            "signal.signal(signal.SIGTERM, terminate)\n"
            "os.write(ready_fd, b'ready\\n')\n"
            "signal.pause()\n"
        )

        def read_line() -> bytes:
            payload = b""
            while not payload.endswith(b"\n"):
                readable, _, _ = select.select([read_fd], [], [], 5)
                if not readable:
                    raise AssertionError("timed out waiting for native child readiness")
                chunk = os.read(read_fd, 1)
                if not chunk:
                    raise AssertionError("native child closed readiness pipe before reporting")
                payload += chunk
            return payload

        def timed_out_probe(*_args, run, **_kwargs):
            run(
                [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                cwd=str(self.root), env=dict(os.environ), text=True,
                capture_output=True, timeout=0.05,
            )
            self.fail("native timeout child unexpectedly returned")

        original_cleanup = executor._bounded_timeout_cleanup

        def catchable_cleanup(child):
            self.assertEqual(read_line(), b"ready\n")
            executor._signal_owned_group(child, signal.SIGTERM)
            process_ids["descendant"] = int(read_line())
            return original_cleanup(child)

        failure = None
        try:
            with patch.object(executor.native_oracle, "probe_materialized_native",
                              side_effect=timed_out_probe), \
                 patch.object(executor, "_bounded_timeout_cleanup", side_effect=catchable_cleanup), \
                 patch.object(executor, "_TERMINATION_GRACE_SECONDS", 0.05):
                try:
                    executor._run_native(
                        self.run_dir, journal, release, docs, config,
                        self.cache, self.sources, subprocess.run,
                    )
                except BaseException as exc:
                    failure = exc
            self.assertIsInstance(failure, executor.OwnedChildCleanupIncomplete)
            persisted = json.loads((self.run_dir / "execution-status.json").read_text())
            self.assertEqual(persisted["phase"], "running")
            self.assertEqual(persisted["releases"][release]["stages"]["native"], "running")
            self.assertIsInstance(persisted["active"]["child"]["pid"], int)
            self.assertFalse(executor._group_live(persisted["active"]["child"]["pgid"]))
            self.assertTrue(executor._pid_live(process_ids["descendant"]))
        finally:
            os.close(read_fd)
            os.close(write_fd)
            descendant = process_ids.get("descendant")
            if descendant is not None:
                try:
                    os.kill(descendant, signal.SIGKILL)
                except ProcessLookupError:
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
            with self.assertRaisesRegex(executor.Refused, "not held"):
                executor.execute(
                    self.run_dir, self.repository, self.cache, self.sources,
                    run=self.command, checkout=self.checkout, host_lock_fd=stream.fileno(),
                )
        with executor._HostLock(self.lock) as held, \
             patch.object(executor.native_oracle, "probe_materialized_native", side_effect=self.probe):
            capability = getattr(held, "capability", None)
            self.assertIsNotNone(capability, "held owner must lend a lock capability")
            journal = executor.execute(
                self.run_dir, self.repository, self.cache, self.sources,
                run=self.command, checkout=self.checkout, host_lock_fd=capability,
            )
        self.assertEqual(journal["phase"], "complete")

    def test_unlocked_external_descriptor_is_refused_without_leaking_a_lock(self):
        self.initialize(self.config())
        self.lock.touch()
        with self.lock.open("r+") as stream:
            with self.assertRaisesRegex(executor.Refused, "not held"):
                executor.execute(
                    self.run_dir, self.repository, self.cache, self.sources,
                    run=self.command, checkout=self.checkout, host_lock_fd=stream.fileno(),
                )
            with executor._HostLock(self.lock):
                pass

    def test_unlocked_stream_cannot_forge_an_external_lock_capability(self):
        self.lock.touch()
        with self.lock.open("r+") as stream:
            with self.assertRaisesRegex(executor.Refused, "issued by lock acquisition"):
                executor._HeldHostLock(self.lock, stream)
            # Rejection must not accidentally acquire or strand the host lock.
            with executor._HostLock(self.lock):
                pass

    def test_external_capability_refuses_after_owning_lock_releases(self):
        self.lock.touch()
        with executor._HostLock(self.lock) as held:
            capability = getattr(held, "capability", None)
            self.assertIsNotNone(capability, "held owner must lend a lock capability")
        with self.assertRaisesRegex(executor.Refused, "not held"):
            with executor._external_host_lock({"host_lock": str(self.lock)}, capability):
                pass

    def test_external_capability_refuses_different_configured_path(self):
        self.lock.touch()
        other = self.root / "other-host.lock"
        other.touch()
        with executor._HostLock(self.lock) as held:
            capability = getattr(held, "capability", None)
            self.assertIsNotNone(capability, "held owner must lend a lock capability")
            with self.assertRaisesRegex(executor.Refused, "differs from configured lease"):
                with executor._external_host_lock({"host_lock": str(other)}, capability):
                    pass

    def test_exited_child_group_residue_gets_a_bounded_grace_then_fails_closed(self):
        """macOS reports EPERM for killpg on a group left only with unreaped zombies."""
        class Exited:
            pid = 424242
            def poll(self):
                return 0
        for label, residue_polls, expected in (("clears within grace", 3, []), ("never clears", None, 1)):
            with self.subTest(label=label):
                owned = executor._OwnedChildren()
                owned.children[Exited.pid] = Exited()
                calls = []
                def killpg(pgid, _signal):
                    calls.append(pgid)
                    if residue_polls is None or len(calls) <= residue_polls:
                        raise PermissionError(1, "Operation not permitted")
                    raise ProcessLookupError()
                self.lock.touch()
                with patch.object(executor, "_OWNED", owned), \
                     patch.object(executor.os, "killpg", side_effect=killpg), \
                     patch.object(executor, "_TERMINATION_GRACE_SECONDS", 0.3):
                    stream = self.lock.open("r+")
                    capability = executor._HeldHostLock.acquire(self.lock, stream)
                    began = __import__("time").monotonic()
                    survivors = executor.release_or_retain(stream, capability)
                    elapsed = __import__("time").monotonic() - began
                self.assertLess(elapsed, 2)
                if expected == []:
                    self.assertEqual(survivors, [])
                    stream.close()
                else:
                    self.assertEqual(len(survivors), expected)
                    self.assertIn("cannot be inspected", survivors[0]["state"])
                    self.assertGreaterEqual(elapsed, 0.3)
                    with patch.object(executor, "_OWNED", executor._OwnedChildren()):
                        self.assertEqual(executor.release_retained_locks(), [])

    def test_standalone_host_lock_is_retained_while_owned_child_is_live(self):
        """No explicit unlock may release the OFD a live child inherited."""
        contender = ("import fcntl, sys\nwith open(sys.argv[1], 'r+') as s:\n"
                     "    try: fcntl.flock(s.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)\n"
                     "    except BlockingIOError: print('blocked')\n    else: print('acquired')\n")
        def contend():
            return subprocess.run([sys.executable, "-c", contender, str(self.lock)], capture_output=True,
                                  text=True, timeout=20, check=True).stdout.strip()
        self.lock.touch()
        child = None
        with patch.object(executor, "_OWNED", executor._OwnedChildren()):
            try:
                with self.assertRaisesRegex(executor.LockRetained, "intentionally still held") as raised:
                    with executor._HostLock(self.lock):
                        child = executor._spawn([sys.executable, "-c", "import time; time.sleep(60)"],
                                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                                start_new_session=True, close_fds=False)
                self.assertIn(f"PID {child.pid}", str(raised.exception))
                self.assertEqual(contend(), "blocked")
            finally:
                if child is not None:
                    child.kill()
                    child.wait(10)
            self.assertEqual(executor.release_retained_locks(), [])
        self.assertEqual(contend(), "acquired")

    def test_between_stage_interruption_is_recoverable(self):
        """Interrupted after stage_passed/checkout_completed: running with nothing active."""
        for boundary in ("checkout_completed", "stage_passed"):
            with self.subTest(boundary=boundary):
                self.run_dir = self.root / f"between-{boundary}"
                self.initialize(self.config())
                status = self.run_dir / "execution-status.json"
                journal = json.loads(status.read_text())
                release = self.releases[0]
                journal["phase"], journal["active"] = "running", None
                if boundary == "stage_passed":
                    journal["releases"][release]["stages"]["native"] = "passed"
                journal["events"].append({"event": boundary, "release": release})
                status.write_text(json.dumps(journal))
                before = json.loads(status.read_text())["releases"]
                recovered = executor.recover(self.run_dir, self.cache, self.sources)
                self.assertEqual(recovered["phase"], "interrupted")
                self.assertIsNone(recovered["active"])
                self.assertEqual(recovered["releases"], before)  # no stage was in flight
                self.assertEqual(recovered["events"][-1]["event"], "interrupted_between_stages")
                with self.assertRaisesRegex(executor.Refused, "terminal"):
                    executor.execute(self.run_dir, self.repository, self.cache, self.sources,
                                     run=self.command, checkout=self.checkout)

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
            executor.recover(self.run_dir, self.cache, self.sources, host_lock_fd=held.capability)

    def test_procfs_liveness_ignores_zombies_and_fails_closed_on_bad_state(self):
        proc_root = self.root / "proc"
        proc_root.mkdir()
        pgid = 4100

        def stat(pid: int, state: str, group: int = pgid) -> None:
            directory = proc_root / str(pid)
            (directory / "task" / str(pid)).mkdir(parents=True, exist_ok=True)
            for target in (directory / "stat", directory / "task" / str(pid) / "stat"):
                target.write_text(f"{pid} (worker name) {state} 1 {group} {group} 0 0 0 0 0\n")

        stat(pgid, "Z")
        stat(pgid + 1, "Z")
        with patch.object(executor.sys, "platform", "linux"), \
             patch.object(executor.os, "killpg"), patch.object(executor.os, "kill"):
            self.assertFalse(executor._pid_live(pgid, proc_root=proc_root))
            self.assertFalse(executor._group_live(pgid, proc_root=proc_root))
            stat(pgid + 1, "S")
            self.assertTrue(executor._group_live(pgid, proc_root=proc_root))
            (proc_root / str(pgid + 1) / "stat").write_text("malformed\n")
            self.assertTrue(executor._group_live(pgid, proc_root=proc_root))

    def test_procfs_zombie_leader_with_a_live_thread_is_live(self):
        """A `Z` main thread does not prove the process gone (#919's Linux caveat).

        When a thread-group leader exits before its other threads, procfs
        shows the leader as a zombie while a live thread still holds every
        inherited descriptor. Liveness must consult each task.
        """
        proc_root = self.root / "proc"
        pgid = 4200

        def task(pid: int, tid: int, state: str) -> None:
            directory = proc_root / str(pid) / "task" / str(tid)
            directory.mkdir(parents=True, exist_ok=True)
            (directory / "stat").write_text(f"{tid} (worker) {state} 1 {pgid} {pgid} 0 0 0 0 0\n")

        (proc_root / str(pgid)).mkdir(parents=True)
        (proc_root / str(pgid) / "stat").write_text(f"{pgid} (worker) Z 1 {pgid} {pgid} 0 0 0 0 0\n")
        task(pgid, pgid, "Z")
        task(pgid, pgid + 1, "S")
        with patch.object(executor.sys, "platform", "linux"), \
             patch.object(executor.os, "killpg"), patch.object(executor.os, "kill"):
            self.assertTrue(executor._pid_live(pgid, proc_root=proc_root))
            self.assertTrue(executor._group_live(pgid, proc_root=proc_root))
            task(pgid, pgid + 1, "Z")
            self.assertFalse(executor._pid_live(pgid, proc_root=proc_root))
            self.assertFalse(executor._group_live(pgid, proc_root=proc_root))
            (proc_root / str(pgid) / "task" / str(pgid + 1) / "stat").write_text("malformed\n")
            self.assertTrue(executor._pid_live(pgid, proc_root=proc_root))

    def test_descendants_fall_back_to_procfs_scan_and_refuse_untrusted_enumeration(self):
        proc_root = self.root / "proc"
        proc_root.mkdir()

        def stat(pid: int, ppid: int, start: int) -> None:
            directory = proc_root / str(pid)
            directory.mkdir()
            fields = ["S", str(ppid), str(pid), str(pid)] + ["0"] * 15 + [str(start)]
            (directory / "stat").write_text(f"{pid} (worker) " + " ".join(fields) + "\n")

        stat(4100, 1, 101)
        stat(4101, 4100, 102)
        with patch.object(executor.sys, "platform", "linux"):
            self.assertEqual(executor._descendants(4100, proc_root=proc_root), [4101])
            with self.assertRaisesRegex(OSError, "enumerat"):
                executor._descendants(4100, proc_root=self.root / "missing-proc")

    def test_descendant_identity_mismatch_is_not_signalled_as_owned(self):
        child = type("Child", (), {"pid": 4100})()
        child._oxidex_owned_descendants = {4101: "start-101"}
        with patch.object(executor, "_process_identity", return_value="start-202"), \
             patch.object(executor, "_signal_pid") as signal_pid:
            self.assertEqual(executor._live_owned_descendants(child), [])
            executor._signal_owned_descendants(child, signal.SIGKILL)
        signal_pid.assert_not_called()

    def test_unverifiable_descendant_identity_fails_closed(self):
        child = type("Child", (), {"pid": 4100})()
        child._oxidex_owned_descendants = {4101: "start-101"}
        with patch.object(executor, "_process_identity", side_effect=OSError("identity unavailable")):
            with self.assertRaisesRegex(OSError, "identity unavailable"):
                executor._live_owned_descendants(child)

    def test_darwin_identity_distinguishes_same_second_kernel_start_times(self):
        coarse_ps = subprocess.CompletedProcess(
            ["ps"], 0, "Sun Sep 21 22:53:08 2026\n", "",
        )
        with patch.object(executor.sys, "platform", "darwin"), \
             patch.object(executor, "_darwin_start_time", create=True,
                          side_effect=[(1_795_000_000, 101), (1_795_000_000, 202)]), \
             patch.object(executor.subprocess, "run", return_value=coarse_ps):
            first = executor._process_identity(4101)
            second = executor._process_identity(4101)
        self.assertNotEqual(first, second)

    def test_darwin_identity_unavailable_fails_closed_instead_of_using_ps(self):
        coarse_ps = subprocess.CompletedProcess(
            ["ps"], 0, "Sun Sep 21 22:53:08 2026\n", "",
        )
        with patch.object(executor.sys, "platform", "darwin"), \
             patch.object(executor, "_darwin_start_time", create=True,
                          side_effect=OSError("libproc unavailable")), \
             patch.object(executor.subprocess, "run", return_value=coarse_ps), \
             self.assertRaisesRegex(OSError, "libproc unavailable"):
            executor._process_identity(4101)

    @unittest.skipUnless(sys.platform == "darwin", "requires macOS libproc")
    def test_actual_darwin_identity_uses_kernel_microsecond_start_time(self):
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        try:
            identity = executor._process_identity(child.pid)
            self.assertRegex(identity, r"^darwin-start:[1-9][0-9]*:[0-9]{1,6}$")
            self.assertEqual(executor._process_identity(child.pid), identity)
        finally:
            child.terminate()
            child.wait(timeout=5)

    def test_untrusted_descendant_enumeration_marks_interrupt_cleanup_incomplete(self):
        child = type("Child", (), {"pid": 4100, "poll": lambda self: None})()
        interruption = KeyboardInterrupt("stop")
        with patch.object(executor, "_bounded_timeout_cleanup", side_effect=OSError("untrusted")), \
             patch.object(executor, "_emergency_reap_group", side_effect=OSError("untrusted")), \
             patch.object(executor, "_refresh_owned_descendants", side_effect=OSError("untrusted")):
            executor._cleanup_owned_child_after_interrupt(child, interruption)
        self.assertEqual(interruption._oxidex_owned_child_cleanup, "incomplete")

    @unittest.skipUnless(sys.platform.startswith("linux"), "requires Linux procfs")
    def test_recovery_accepts_real_zombie_only_group(self):
        self.initialize(self.config())
        child = subprocess.Popen(
            [sys.executable, "-c", "import os; os._exit(0)"],
            start_new_session=True,
        )
        try:
            stat_path = Path("/proc") / str(child.pid) / "stat"
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                raw = stat_path.read_text()
                if raw[raw.rfind(")") + 2:].split()[0] == "Z":
                    break
                time.sleep(0.01)
            else:
                self.fail("real child did not enter zombie state")
            journal_path = self.run_dir / "execution-status.json"
            journal = json.loads(journal_path.read_text())
            release = self.releases[0]
            journal["phase"] = "running"
            journal["active"] = {
                "release": release,
                "stage": "generate",
                "child": {"pid": child.pid, "pgid": child.pid},
            }
            journal["releases"][release]["stages"]["generate"] = "running"
            executor._store_journal(self.run_dir, journal)
            with executor._HostLock(self.lock) as held:
                recovered = executor.recover(
                    self.run_dir, self.cache, self.sources, host_lock_fd=held.capability,
                )
            self.assertEqual(recovered["phase"], "interrupted")
            self.assertIsNone(recovered["active"])
        finally:
            child.wait(timeout=5)

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
                    with self.subTest(process=name):
                        self.assertFalse(executor._pid_live(process_ids[name]))
                recovered = executor.recover(
                    self.run_dir, self.cache, self.sources, host_lock_fd=held.capability,
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

    def test_interrupt_reaps_descendant_that_escapes_session_and_ignores_term(self):
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        ready_error: list[BaseException] = []
        descendant_program = (
            "import json, os, signal, sys, time\n"
            "os.setsid()\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "os.write(int(sys.argv[1]), (json.dumps({'descendant': os.getpid()}) + '\\n').encode())\n"
            "os.close(int(sys.argv[1]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import subprocess, sys\n"
            "descendant = subprocess.Popen([sys.executable, '-c', sys.argv[2], sys.argv[1]], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "close_fds=False)\n"
            "raise SystemExit(descendant.wait())\n"
        )

        def interrupt_when_escaped_descendant_is_ready() -> None:
            try:
                payload = b""
                while not payload.endswith(b"\n"):
                    chunk = os.read(read_fd, 4096)
                    if not chunk:
                        raise AssertionError("escaped descendant closed readiness pipe before reporting")
                    payload += chunk
                process_ids.update(json.loads(payload))
                os.kill(os.getpid(), signal.SIGINT)
            except BaseException as exc:
                ready_error.append(exc)

        def started(pid: int, pgid: int) -> None:
            process_ids.update(direct=pid, pgid=pgid)
            os.close(write_fd)

        interrupter = threading.Thread(
            target=interrupt_when_escaped_descendant_is_ready,
            daemon=True,
        )
        try:
            with executor._HostLock(self.lock), \
                 patch.object(executor, "_TERMINATION_GRACE_SECONDS", 0.1):
                interrupter.start()
                with self.assertRaises(KeyboardInterrupt):
                    executor._run_record(
                        [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                        cwd=self.root, env=dict(os.environ), run=subprocess.run, started=started,
                    )
                interrupter.join(timeout=5)
                self.assertFalse(interrupter.is_alive(), "escaped descendant readiness was not observed")
                if ready_error:
                    raise ready_error[0]
                self.assertFalse(executor._pid_live(process_ids["direct"]))
                self.assertFalse(executor._pid_live(process_ids["descendant"]))
            with executor._HostLock(self.lock):
                pass
        finally:
            for descriptor in (write_fd, read_fd):
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            descendant = process_ids.get("descendant")
            if descendant is not None:
                try:
                    os.kill(descendant, signal.SIGKILL)
                except ProcessLookupError:
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

    def test_cleanup_fails_closed_when_term_handler_spawns_escaped_descendant(self):
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        descendant_program = (
            "import os, signal, sys, time\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "while os.getppid() == int(sys.argv[1]):\n"
            "    time.sleep(0.001)\n"
            "os.write(int(sys.argv[2]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[2]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import os, signal, subprocess, sys\n"
            "fd = int(sys.argv[1])\n"
            "def terminate(_signum, _frame):\n"
            "    subprocess.Popen([sys.executable, '-c', sys.argv[2], "
            "str(os.getpid()), str(fd)], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
            "    os.close(fd)\n"
            "    raise SystemExit(0)\n"
            "signal.signal(signal.SIGTERM, terminate)\n"
            "os.write(fd, b'ready\\n')\n"
            "signal.pause()\n"
        )

        def read_line() -> bytes:
            payload = b""
            while not payload.endswith(b"\n"):
                chunk = os.read(read_fd, 1)
                if not chunk:
                    raise AssertionError("owned child closed readiness pipe before reporting")
                payload += chunk
            return payload

        owner: list[subprocess.Popen[str] | None] = [None]
        child = None
        try:
            child = executor._spawn_with_deferred_sigint(
                owner,
                [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                text=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, start_new_session=True, close_fds=False,
            )
            os.close(write_fd)
            write_fd = -1
            self.assertEqual(read_line(), b"ready\n")
            executor._signal_owned_group(child, signal.SIGTERM)
            process_ids["descendant"] = int(read_line())
            interruption = KeyboardInterrupt("original interruption")
            with patch.object(executor, "_TERMINATION_GRACE_SECONDS", 0.05):
                executor._cleanup_owned_child_after_interrupt(child, interruption)
            self.assertEqual(
                getattr(interruption, "_oxidex_owned_child_cleanup", None),
                "incomplete",
            )
            self.assertFalse(executor._group_live(child.pid))
            self.assertTrue(executor._pid_live(process_ids["descendant"]))
        finally:
            if write_fd >= 0:
                os.close(write_fd)
            os.close(read_fd)
            descendant = process_ids.get("descendant")
            if descendant is not None:
                try:
                    os.kill(descendant, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if child is not None:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except OSError:
                    pass
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass

                executor._close_ownership_probe(child)

    def test_interrupt_cleanup_fails_closed_when_term_handler_closes_marker_before_escape(self):
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        process_ids: dict[str, int] = {}
        descendant_program = (
            "import os, signal, sys, time\n"
            "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            "while os.getppid() == int(sys.argv[1]):\n"
            "    time.sleep(0.001)\n"
            "os.write(int(sys.argv[2]), f'{os.getpid()}\\n'.encode())\n"
            "os.close(int(sys.argv[2]))\n"
            "time.sleep(60)\n"
        )
        child_program = (
            "import os, signal, subprocess, sys\n"
            "ready_fd = int(sys.argv[1])\n"
            "def terminate(_signum, _frame):\n"
            "    fd_root = '/dev/fd' if os.path.isdir('/dev/fd') else '/proc/self/fd'\n"
            "    for name in os.listdir(fd_root):\n"
            "        if name.isdigit() and int(name) > 2 and int(name) != ready_fd:\n"
            "            try:\n"
            "                os.close(int(name))\n"
            "            except OSError:\n"
            "                pass\n"
            "    subprocess.Popen([sys.executable, '-c', sys.argv[2], "
            "str(os.getpid()), str(ready_fd)], "
            "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, "
            "start_new_session=True, close_fds=False)\n"
            "    os.close(ready_fd)\n"
            "    raise SystemExit(0)\n"
            "signal.signal(signal.SIGTERM, terminate)\n"
            "os.write(ready_fd, b'ready\\n')\n"
            "signal.pause()\n"
        )

        def read_line() -> bytes:
            payload = b""
            while not payload.endswith(b"\n"):
                chunk = os.read(read_fd, 1)
                if not chunk:
                    raise AssertionError("owned child closed readiness pipe before reporting")
                payload += chunk
            return payload

        owner: list[subprocess.Popen[str] | None] = [None]
        child = None
        try:
            child = executor._spawn_with_deferred_sigint(
                owner,
                [sys.executable, "-c", child_program, str(write_fd), descendant_program],
                text=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, start_new_session=True, close_fds=False,
            )
            os.close(write_fd)
            write_fd = -1
            self.assertEqual(read_line(), b"ready\n")
            executor._signal_owned_group(child, signal.SIGTERM)
            process_ids["descendant"] = int(read_line())
            interruption = KeyboardInterrupt("original interruption")
            with patch.object(executor, "_TERMINATION_GRACE_SECONDS", 0.05):
                executor._cleanup_owned_child_after_interrupt(child, interruption)
            self.assertEqual(
                getattr(interruption, "_oxidex_owned_child_cleanup", None),
                "incomplete",
            )
            self.assertTrue(any("cannot be verified" in note for note in interruption.__notes__))
            self.assertFalse(executor._group_live(child.pid))
            self.assertTrue(executor._pid_live(process_ids["descendant"]))
        finally:
            if write_fd >= 0:
                os.close(write_fd)
            os.close(read_fd)
            descendant = process_ids.get("descendant")
            if descendant is not None:
                try:
                    os.kill(descendant, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if child is not None:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    child.wait(timeout=5)
                except (subprocess.TimeoutExpired, ChildProcessError):
                    pass
                executor._close_ownership_probe(child)

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
        self.assertFalse(executor._pid_live(child_pid))

    @_without_lineage_supervisor
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
            # The surviving group member also keeps the host lock held: the
            # owner's exit must retain it rather than unlock (fail closed).
            with self.assertRaises(executor.LockRetained) as retained, \
                 executor._HostLock(self.lock) as held, \
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
                        self.run_dir, self.cache, self.sources, host_lock_fd=held.capability,
                    )
                self.assertTrue(any("emergency cleanup failed" in note for note in notes))
                emergency.assert_called_once()
            self.assertEqual([item["pid"] for item in retained.exception.survivors], [process_ids["direct"]])
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
            deadline = time.monotonic() + 10
            while executor.release_retained_locks() and time.monotonic() < deadline:
                time.sleep(0.05)

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



ZOMBIE_GROUP_HELPER = r"""
import os, sys, time
b = os.fork()
if b == 0:
    os.setpgid(0, 0)                       # B leads a new group in A's session
    z = os.fork()
    if z == 0:
        os._exit(0)                        # Z exits; B never reaps it
    time.sleep(0.2)
    os.setpgid(0, os.getpgid(os.getppid()))  # B rejoins A's group: group B is zombie Z only
    sys.stdout.write(f"{os.getpid()} {z}\n"); sys.stdout.flush()
    time.sleep(30)
    os._exit(0)
time.sleep(30)
"""


class _Reaped:
    """A registered child already reaped by its Popen (poll() is final)."""

    def __init__(self, pid):
        self.pid = pid

    def poll(self):
        return 0


class ZombieGroupTests(unittest.TestCase):
    """A zombie holds no descriptors, so a zombie-only group cannot hold the flock."""

    @unittest.skipUnless(sys.platform == "darwin", "the zombie-only relaxation is macOS-only")
    def test_zombie_only_group_is_gone_but_a_live_member_is_not(self):
        helper = subprocess.Popen([sys.executable, "-c", ZOMBIE_GROUP_HELPER], stdout=subprocess.PIPE,
                                  text=True, start_new_session=True)
        def cleanup():
            # Only this test's own helper group; EPERM means only zombies remain.
            try:
                os.killpg(helper.pid, __import__("signal").SIGKILL)
            except (ProcessLookupError, PermissionError):
                pass
            helper.wait(10)
            helper.stdout.close()
        self.addCleanup(cleanup)
        leader_b, zombie = map(int, helper.stdout.readline().split())
        self.assertIsNone(executor._unproven_state(_Reaped(leader_b)),
                          "a group holding only zombie members cannot hold the lock")
        self.assertEqual(executor._group_member_states(leader_b), [(zombie, "zombie")])
        # Group A still holds live A and B: never proven gone.
        members = dict(executor._group_member_states(helper.pid))
        self.assertEqual(members, {helper.pid: "live", leader_b: "live"})
        self.assertIn("live members", executor._unproven_state(_Reaped(helper.pid)))
        with TemporaryDirectory() as temporary:
            lock = Path(temporary) / "zombie.lock"; lock.touch()
            owned = executor._OwnedChildren(); owned.children[leader_b] = _Reaped(leader_b)
            with patch.object(executor, "_OWNED", owned):
                stream = lock.open("r+")
                capability = executor._HeldHostLock.acquire(lock, stream)
                began = __import__("time").monotonic()
                self.assertEqual(executor.release_or_retain(stream, capability), [])
                self.assertLess(__import__("time").monotonic() - began, 1)
                stream.close()

    def test_group_classifier_is_fail_closed(self):
        for members, gone in ((None, False), ([], False), ([(1, "zombie"), (2, "live")], False),
                              ([(1, "zombie"), (2, "zombie")], True)):
            with self.subTest(members=members), \
                 patch.object(executor, "_group_member_states", return_value=members):
                self.assertIs(executor._zombie_only_group(7), gone)
        with patch.object(executor.sys, "platform", "freebsd14"):
            self.assertIsNone(executor._group_member_states(7))

    def test_non_darwin_platforms_are_always_unproven(self):
        """/proc shows only a main thread's state and is not a snapshot: never trust it."""
        self.assertFalse(hasattr(executor, "_linux_group_members"), "no re-enableable /proc scan")
        for platform in ("linux", "linux2", "freebsd14", "win32", "cygwin"):
            with self.subTest(platform=platform), patch.object(executor.sys, "platform", platform), \
                 patch.object(executor.os, "killpg", return_value=None):
                self.assertIsNone(executor._group_member_states(7))
                self.assertFalse(executor._zombie_only_group(7))
                self.assertIn("live members", executor._unproven_state(_Reaped(7)))
            with self.subTest(platform=platform, killpg="EPERM"), \
                 patch.object(executor.sys, "platform", platform), \
                 patch.object(executor.os, "killpg", side_effect=PermissionError(1, "Operation not permitted")):
                self.assertIn("cannot be inspected", executor._unproven_state(_Reaped(7)))

    def test_darwin_rescan_must_confirm_the_same_zombie_members(self):
        zombie = [(10, "zombie")]
        for label, rescan, gone in (("membership grew", [(10, "zombie"), (11, "zombie")], False),
                                    ("member revived state", [(10, "live")], False),
                                    ("rescan failed", None, False),
                                    ("member replaced", [(12, "zombie")], False),
                                    ("identical", [(10, "zombie")], True)):
            with self.subTest(label=label), patch.object(executor.sys, "platform", "darwin"), \
                 patch.object(executor, "_group_member_states", side_effect=[zombie, rescan]) as states:
                self.assertIs(executor._zombie_only_group(10), gone)
                self.assertEqual(states.call_count, 2)
        with patch.object(executor.sys, "platform", "darwin"), \
             patch.object(executor, "_group_member_states", side_effect=[[(10, "live")]]) as states:
            self.assertFalse(executor._zombie_only_group(10))
            self.assertEqual(states.call_count, 1)  # no re-scan once a live member is seen

    @unittest.skipUnless(sys.platform == "darwin", "macOS libproc/sysctl prototypes")
    def test_darwin_ctypes_prototypes_are_declared(self):
        import ctypes
        libc, libproc = executor._darwin_libraries()
        self.assertEqual(libc.sysctl.argtypes, [ctypes.POINTER(ctypes.c_int), ctypes.c_uint, ctypes.c_void_p,
                                                ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p, ctypes.c_size_t])
        self.assertIs(libc.sysctl.restype, ctypes.c_int)
        self.assertEqual(libproc.proc_listpids.argtypes,
                         [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_int])
        self.assertIs(libproc.proc_listpids.restype, ctypes.c_int)

    def test_darwin_views_must_agree_and_layout_must_check(self):
        me = os.getpid()
        def kinfo(listing, group):
            def view(mib):
                return [(me, 2)] if mib[2] == 1 else group
            return view
        cases = (([10, 11], [(10, 5)], None),           # the two kernel views disagree
                 ([10], [(10, 5)], [(10, "zombie")]),
                 ([10], [(10, 2)], [(10, "live")]),
                 ([10], None, None))                      # sysctl failed
        for listing, group, expected in cases:
            with self.subTest(listing=listing, group=group), \
                 patch.object(executor, "_darwin_pgrp_listpids", return_value=listing), \
                 patch.object(executor, "_darwin_kinfo", side_effect=kinfo(listing, group)):
                self.assertEqual(executor._darwin_group_members(10), expected)
        with patch.object(executor, "_darwin_pgrp_listpids", return_value=[10]), \
             patch.object(executor, "_darwin_kinfo", side_effect=lambda mib: [(me + 1, 2)] if mib[2] == 1 else [(10, 5)]):
            self.assertIsNone(executor._darwin_group_members(10))  # layout self-check failed

if __name__ == "__main__":
    unittest.main()
