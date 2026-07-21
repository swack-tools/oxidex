import io
import signal
import unittest

from stop_parallel_fix import main, wait_until_dead


class WaitUntilDeadTests(unittest.TestCase):
    def test_returns_empty_when_all_die_immediately(self):
        result = wait_until_dead(
            [1, 2], timeout=5,
            is_alive_fn=lambda pid: False,
            sleep_fn=lambda s: None,
            now_fn=self._clock([0, 1]),
        )
        self.assertEqual(result, [])

    def test_returns_still_alive_pids_after_timeout_elapses(self):
        # is_alive_fn never returns False -- simulates a process that
        # ignores SIGTERM entirely, so the poll loop must give up once
        # now_fn's clock passes the deadline, not spin forever.
        result = wait_until_dead(
            [1, 2], timeout=5,
            is_alive_fn=lambda pid: True,
            sleep_fn=lambda s: None,
            now_fn=self._clock([0, 1, 2, 3, 4, 5, 6, 7]),
        )
        self.assertEqual(result, [1, 2])

    def test_polls_at_the_given_interval(self):
        sleeps = []
        wait_until_dead(
            [1], timeout=1,
            is_alive_fn=lambda pid: True,
            sleep_fn=sleeps.append,
            now_fn=self._clock([0, 0.5, 1, 1.5]),
        )
        self.assertTrue(all(s == 0.2 for s in sleeps))

    @staticmethod
    def _clock(values):
        it = iter(values)
        return lambda: next(it)


class MainTests(unittest.TestCase):
    def _run(self, argv, find_pids_map, alive_pids=None, command_lines=None):
        """find_pids_map: {pattern: [pids]}. alive_pids: set of pids that
        stay alive when polled (defaults to none -- everything dies
        immediately after being signaled, matching the common case)."""
        alive_pids = set(alive_pids or ())
        command_lines = command_lines or {}
        signaled = []  # (pid, sig, "pid" or "group")

        def find_pids_fn(pattern):
            return list(find_pids_map.get(pattern, []))

        def command_line_fn(pid):
            return command_lines.get(pid, f"fake-command-{pid}")

        def is_alive_fn(pid):
            return pid in alive_pids

        def signal_pid_fn(pid, sig):
            signaled.append((pid, sig, "pid"))
            alive_pids.discard(pid)

        def signal_group_fn(pid, sig):
            signaled.append((pid, sig, "group"))
            alive_pids.discard(pid)

        out = io.StringIO()
        exit_code = main(
            argv, find_pids_fn=find_pids_fn, command_line_fn=command_line_fn,
            is_alive_fn=is_alive_fn, signal_pid_fn=signal_pid_fn, signal_group_fn=signal_group_fn,
            sleep_fn=lambda s: None, now_fn=self._clock(), stdout=out,
        )
        return exit_code, out.getvalue(), signaled

    @staticmethod
    def _clock():
        t = [0.0]

        def now():
            t[0] += 0.3
            return t[0]

        return now

    def test_nothing_running_signals_nothing(self):
        exit_code, output, signaled = self._run([], find_pids_map={})
        self.assertEqual(exit_code, 0)
        self.assertIn("Nothing running", output)
        self.assertEqual(signaled, [])

    def test_wrapper_signaled_by_pid_not_group(self):
        exit_code, output, signaled = self._run(
            [], find_pids_map={"parallel_tag_fix_loop.py": [100], "parallel_model_fix_loop.py": []},
        )
        self.assertEqual(exit_code, 0)
        self.assertEqual(signaled, [(100, signal.SIGTERM, "pid")])
        self.assertIn("1 wrapper process(es)", output)

    def test_worker_signaled_by_group_not_pid(self):
        exit_code, output, signaled = self._run(
            [], find_pids_map={"model_fix_loop.py": [200, 201]},
        )
        self.assertEqual(signaled, [(200, signal.SIGTERM, "group"), (201, signal.SIGTERM, "group")])
        self.assertIn("2 worker process(es)", output)

    def test_wrapper_pids_excluded_from_worker_pids_despite_substring_overlap(self):
        # "parallel_model_fix_loop.py" contains "model_fix_loop.py" as a
        # substring, so a pgrep -f for the worker pattern would also
        # match the older wrapper's own process -- it must not be
        # double-counted (and double-signaled) as a worker too.
        exit_code, output, signaled = self._run(
            [],
            find_pids_map={
                "parallel_tag_fix_loop.py": [],
                "parallel_model_fix_loop.py": [100],
                "model_fix_loop.py": [100, 200],
            },
        )
        # pid 100 signaled once, as a wrapper (pid-scoped) -- not again
        # as a worker (group-scoped).
        self.assertEqual(signaled.count((100, signal.SIGTERM, "pid")), 1)
        self.assertNotIn((100, signal.SIGTERM, "group"), signaled)
        self.assertIn((200, signal.SIGTERM, "group"), signaled)

    def test_dry_run_signals_nothing(self):
        exit_code, output, signaled = self._run(
            ["--dry-run"],
            find_pids_map={"parallel_tag_fix_loop.py": [100], "model_fix_loop.py": [200]},
        )
        self.assertEqual(exit_code, 0)
        self.assertEqual(signaled, [])
        self.assertIn("Would signal", output)
        self.assertIn("Dry run", output)

    def test_clean_exit_does_not_escalate(self):
        exit_code, output, signaled = self._run(
            [], find_pids_map={"parallel_tag_fix_loop.py": [100]},
        )
        self.assertNotIn("SIGKILL", output)
        self.assertIn(signal.SIGTERM, [sig for _pid, sig, _kind in signaled])


class SigtermEscalationTests(unittest.TestCase):
    """A dedicated fixture for the escalation path: signal_pid_fn/
    signal_group_fn must NOT clear aliveness on SIGTERM (only SIGKILL
    does, in reality) to actually exercise wait_until_dead's timeout."""

    def test_ignored_sigterm_escalates_to_sigkill_only_once(self):
        alive_pids = {100}
        signaled = []

        def find_pids_fn(pattern):
            return [100] if pattern == "parallel_tag_fix_loop.py" else []

        def signal_pid_fn(pid, sig):
            signaled.append((pid, sig))
            if sig == signal.SIGKILL:
                alive_pids.discard(pid)

        t = [0.0]

        def now_fn():
            t[0] += 1.0
            return t[0]

        out = io.StringIO()
        exit_code = main(
            ["--grace-period", "2"],
            find_pids_fn=find_pids_fn, command_line_fn=lambda pid: "fake",
            is_alive_fn=lambda pid: pid in alive_pids,
            signal_pid_fn=signal_pid_fn, signal_group_fn=lambda pid, sig: None,
            sleep_fn=lambda s: None, now_fn=now_fn, stdout=out,
        )
        self.assertEqual(exit_code, 0)
        self.assertIn((100, signal.SIGTERM), signaled)
        self.assertIn((100, signal.SIGKILL), signaled)
        self.assertNotIn(100, alive_pids)
        self.assertIn("escalating to SIGKILL", out.getvalue())


if __name__ == "__main__":
    unittest.main()
