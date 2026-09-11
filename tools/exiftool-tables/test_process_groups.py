"""Owned process controls plus injected permission-denial controls; no Cargo."""
import errno
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import process_groups as groups


class ProcessGroupsTests(unittest.TestCase):
    def launch(self, code):
        proc = subprocess.Popen([sys.executable, '-c', code], start_new_session=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.cleanup_owned, proc)
        return proc

    @staticmethod
    def cleanup_owned(proc):
        try:
            groups.cleanup_process_group(proc, term_timeout=.05, kill_timeout=2)
        finally:
            if proc.stdout:
                proc.stdout.close()
            if proc.stderr:
                proc.stderr.close()

    def test_exited_command_status_survives_descendant_cleanup(self):
        proc = self.launch("import subprocess,sys; p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(5)'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); print(p.pid,flush=True); sys.exit(7)")
        child = int(proc.stdout.readline())
        self.assertEqual(proc.wait(timeout=2), 7)
        report = groups.cleanup_process_group(proc, term_timeout=.2, kill_timeout=2)
        self.assertEqual(report['returncode'], 7)
        self.assertEqual(report['status'], 'gone')
        with self.assertRaises(ProcessLookupError):
            os.killpg(proc.pid, 0)

    def test_term_resistant_descendant_gets_actual_grace_then_kill(self):
        child = "import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); print('ready',flush=True); time.sleep(5)"
        parent = f"import subprocess,sys; p=subprocess.Popen([sys.executable,'-c',{child!r}],stdout=subprocess.PIPE,text=True); p.stdout.readline(); print(p.pid,flush=True); sys.exit(9)"
        proc = self.launch(parent)
        int(proc.stdout.readline())
        self.assertEqual(proc.wait(timeout=2), 9)
        report = groups.cleanup_process_group(proc, term_timeout=.1, kill_timeout=2)
        self.assertEqual([e['signal'] for e in report['signals']], ['SIGTERM', 'SIGKILL'])
        self.assertGreaterEqual(report['signals'][1]['at'] - report['signals'][0]['at'], .09)
        self.assertEqual(report['returncode'], 9)

    def test_transient_eperm_requires_observed_group_disappearance(self):
        proc = mock.Mock(pid=123456, returncode=7)
        calls = []
        def killpg(pgid, sig):
            calls.append(sig)
            if len(calls) < 4:
                raise PermissionError(errno.EPERM, 'simulated dying group')
            raise ProcessLookupError(errno.ESRCH, 'observed gone')
        with mock.patch.object(groups.os, 'killpg', side_effect=killpg):
            report = groups.cleanup_process_group(proc, term_timeout=.1, kill_timeout=.1, poll_interval=.001)
        self.assertEqual(report['status'], 'gone')
        self.assertEqual(report['returncode'], 7)
        self.assertGreater(report['permission_errors'], 0)
        self.assertEqual(calls[-1], 0)

    def test_persistent_permission_failure_is_not_ignored(self):
        proc = mock.Mock(pid=123456, returncode=23)
        with mock.patch.object(groups.os, 'killpg', side_effect=PermissionError(errno.EPERM, 'denied')):
            with self.assertRaises(groups.GroupCleanupError) as caught:
                groups.cleanup_process_group(proc, term_timeout=.002, kill_timeout=.002, poll_interval=.001)
        self.assertEqual(caught.exception.report['returncode'], 23)
        self.assertEqual(caught.exception.report['status'], 'cleanup-failed')
        self.assertIn('SIGKILL', [e['signal'] for e in caught.exception.report['signals']])

    def test_unknown_os_error_is_not_discarded(self):
        proc = mock.Mock(pid=123456, returncode=7)
        with mock.patch.object(groups.os, 'killpg', side_effect=OSError(errno.EIO, 'injected I/O failure')):
            with self.assertRaises(OSError) as caught:
                groups.cleanup_process_group(proc)
        self.assertEqual(caught.exception.errno, errno.EIO)

    def test_term_interrupt_cleans_new_session_descendants(self):
        helper = str(Path(groups.__file__).parent)
        code = f'''import json,subprocess,sys,time
sys.path.insert(0,{helper!r})
from process_groups import interruptible,cleanup_process_group,ProcessInterrupted
p=None
try:
    with interruptible():
        try:
            p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(5)'],start_new_session=True)
            print(json.dumps({{"child":p.pid}}),flush=True)
            p.wait()
        finally:
            if p is not None: cleanup_process_group(p,term_timeout=.05,kill_timeout=2)
except ProcessInterrupted as e:
    print('interrupted',e.signum,flush=True)
    sys.exit(128+e.signum)
'''
        proc = self.launch(code)
        child = json.loads(proc.stdout.readline())['child']
        proc.send_signal(signal.SIGTERM)
        stdout, stderr = proc.communicate(timeout=4)
        self.assertEqual(proc.returncode, 143, stderr)
        self.assertIn('interrupted 15', stdout)
        with self.assertRaises(ProcessLookupError):
            os.killpg(child, 0)

    def test_signal_handlers_are_scoped_and_custom_handlers_preserved(self):
        previous = signal.getsignal(signal.SIGTERM)
        try:
            signal.signal(signal.SIGTERM, signal.SIG_DFL)
            with groups.interruptible():
                self.assertNotEqual(signal.getsignal(signal.SIGTERM), signal.SIG_DFL)
            self.assertEqual(signal.getsignal(signal.SIGTERM), signal.SIG_DFL)
            custom = lambda _signum, _frame: None
            signal.signal(signal.SIGTERM, custom)
            with groups.interruptible():
                self.assertIs(signal.getsignal(signal.SIGTERM), custom)
            self.assertIs(signal.getsignal(signal.SIGTERM), custom)
        finally:
            signal.signal(signal.SIGTERM, previous)


    def test_captured_nonzero_command_keeps_exit_and_output(self):
        result = groups.run_captured([sys.executable, '-c',
            "import sys; print('out'); print('problem',file=sys.stderr); sys.exit(23)"], timeout=2)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (23, 'out\n', 'problem\n'))

    def test_captured_timeout_cleans_child_and_preserves_timeout(self):
        code = "import os,time; print(os.getpid(),flush=True); time.sleep(5)"
        with self.assertRaises(subprocess.TimeoutExpired) as caught:
            groups.run_captured([sys.executable, '-c', code], timeout=.1)
        pid = int(caught.exception.output.strip())
        with self.assertRaises(ProcessLookupError):
            os.killpg(pid, 0)

    def test_failed_command_and_cleanup_failure_are_both_reported(self):
        report = {'pgid': 123456, 'permission_errors': 1, 'returncode': 23}
        error = groups.GroupCleanupError(report)
        with mock.patch.object(groups, 'terminate_group', side_effect=error):
            with self.assertRaises(subprocess.CalledProcessError) as caught:
                groups.run_captured([sys.executable, '-c', 'import sys; sys.exit(23)'], timeout=2)
        self.assertEqual(caught.exception.returncode, 23)
        self.assertIs(caught.exception.cleanup_error, error)
        self.assertIn('cleanup also failed', caught.exception.__notes__[0])

    def test_keyboard_interrupt_cleans_the_owned_process_before_propagating(self):
        real_popen = subprocess.Popen
        created = []
        interrupted = KeyboardInterrupt('controlled interrupt')
        def spawn(*args, **kwargs):
            proc = real_popen(*args, **kwargs)
            proc.communicate = mock.Mock(side_effect=interrupted)
            created.append(proc)
            self.addCleanup(self.cleanup_owned, proc)
            return proc
        with mock.patch.object(groups.subprocess, 'Popen', side_effect=spawn):
            with self.assertRaises(KeyboardInterrupt) as caught:
                groups.run_captured([sys.executable, '-c', 'import time; time.sleep(5)'], timeout=2)
        self.assertIs(caught.exception, interrupted)
        with self.assertRaises(ProcessLookupError):
            os.killpg(created[0].pid, 0)

    def test_timeout_is_not_masked_by_cleanup_failure(self):
        real_popen = subprocess.Popen
        created = []
        def spawn(*args, **kwargs):
            proc = real_popen(*args, **kwargs)
            created.append(proc)
            self.addCleanup(self.cleanup_owned, proc)
            return proc
        error = groups.GroupCleanupError({'pgid': 123456, 'permission_errors': 1, 'returncode': None})
        with mock.patch.object(groups.subprocess, 'Popen', side_effect=spawn):
            with mock.patch.object(groups, 'terminate_group', side_effect=error):
                with self.assertRaises(subprocess.TimeoutExpired) as caught:
                    groups.run_captured([sys.executable, '-c', 'import time; time.sleep(5)'], timeout=.05)
        self.assertIs(caught.exception.cleanup_error, error)
        self.assertEqual(len(created), 1)

    def test_outer_group_allows_inner_term_resistant_group_to_finish_cleanup(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / 'ready.json'
            grandchild = "import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); print('ready',flush=True); time.sleep(8)"
            inner = ("import json,os,pathlib,subprocess,sys,time; "
                     f"p=subprocess.Popen([sys.executable,'-c',{grandchild!r}],stdout=subprocess.PIPE,text=True); "
                     "p.stdout.readline(); "
                     f"pathlib.Path({str(marker)!r}).write_text(json.dumps({{'leader':os.getpid(),'grandchild':p.pid}})); "
                     "time.sleep(8)")
            supervisor = ("import sys; "
                          f"sys.path.insert(0,{str(Path(groups.__file__).parent)!r}); "
                          "from process_groups import run_captured,ProcessInterrupted\n"
                          "try:\n"
                          f"    run_captured([sys.executable,'-c',{inner!r}],timeout=10)\n"
                          "except ProcessInterrupted as e:\n"
                          "    sys.exit(128+e.signum)\n")
            proc = self.launch(supervisor)
            deadline = time.monotonic() + 3
            while not marker.exists() and time.monotonic() < deadline:
                time.sleep(.01)
            self.assertTrue(marker.exists(), 'inner group must reach its readiness sentinel')
            child = json.loads(marker.read_text())
            groups.terminate_group(proc, term_timeout=5, kill_timeout=2)
            self.assertEqual(proc.returncode, 143)
            with self.assertRaises(ProcessLookupError):
                os.killpg(child['leader'], 0)


if __name__ == '__main__':
    unittest.main()
