"""Bounded cleanup for subprocesses created with start_new_session=True.

Waiting for the command leader does not wait for descendants. In particular,
Darwin reports EPERM for a process group containing only unreaped zombies.
Accept EPERM only when a subsequent probe observes ESRCH (the group is gone).
The caller must retain its command exception/return code alongside cleanup errors.
"""
from contextlib import contextmanager
import errno
import os
import signal
import subprocess
import threading
import time


class GroupCleanupError(OSError):
    def __init__(self, report):
        self.report = report
        super().__init__(errno.EPERM if report['permission_errors'] else errno.EBUSY,
                         f"process group {report['pgid']} did not disappear after bounded cleanup; "
                         f"command return code={report['returncode']}; "
                         f"permission errors={report['permission_errors']}")


class ProcessInterrupted(RuntimeError):
    # selectors catches InterruptedError as an ordinary EINTR and retries it;
    # cancellation must use a distinct exception that reaches caller finally.
    def __init__(self, signum):
        self.signum = signum
        super().__init__(f"interrupted by signal {signum}")


@contextmanager
def interruptible():
    """Make default TERM/HUP raise so a caller's finally cleans nested groups.

    Custom handlers (including the transaction's Refused handler), ignored
    signals and Python's default KeyboardInterrupt behavior are preserved.
    Use around spawning, waiting AND cleanup, then restore entry handlers.
    """
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError('interruptible subprocess orchestration requires the main thread')
    previous = {}
    def interrupted(signum, _frame):
        raise ProcessInterrupted(signum)
    try:
        for signum in (signal.SIGTERM, signal.SIGHUP):
            handler = signal.getsignal(signum)
            if handler == signal.SIG_DFL:
                previous[signum] = handler
                signal.signal(signum, interrupted)
        yield
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def cleanup_process_group(proc, *, term_timeout=2.0, kill_timeout=2.0, poll_interval=0.02):
    """TERM, allow the actual group to drain, KILL if needed, and verify gone.

    Returns a diagnostic report including the leader's actual return code.
    Persistent EPERM or any group still present at the deadline fails closed.
    Never use this for a process not created by this caller in a new session.
    """
    if min(term_timeout, kill_timeout) < 0 or poll_interval <= 0:
        raise ValueError('cleanup timeouts must be nonnegative and polling positive')
    pgid = proc.pid
    if pgid <= 1 or pgid == os.getpgrp():
        raise ValueError('refusing cleanup of a non-private process group')
    report = {'pgid': pgid, 'signals': [], 'permission_errors': 0, 'returncode': proc.returncode}

    def exists():
        # Reap the directly owned leader even when descendants outlive it.
        proc.poll()
        report['returncode'] = proc.returncode
        try:
            os.killpg(pgid, 0)
            return True
        except ProcessLookupError:
            return False
        except PermissionError:
            report['permission_errors'] += 1
            return True  # Unknown/dying is not proof of gone.

    def send(signum):
        event = {'signal': signum.name, 'at': time.monotonic()}
        report['signals'].append(event)
        try:
            os.killpg(pgid, signum)
        except ProcessLookupError:
            event['errno'] = errno.ESRCH
        except PermissionError:
            event['errno'] = errno.EPERM
            report['permission_errors'] += 1

    def wait_gone(seconds):
        deadline = time.monotonic() + seconds
        while exists():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return False
            time.sleep(min(poll_interval, remaining))
        return True

    if not exists():
        report['status'] = 'gone'
        return report
    send(signal.SIGTERM)
    if wait_gone(term_timeout):
        report['status'] = 'gone'
        return report
    send(signal.SIGKILL)
    if wait_gone(kill_timeout):
        report['status'] = 'gone'
        return report
    report['status'] = 'cleanup-failed'
    raise GroupCleanupError(report)


def terminate_group(proc, *, term_timeout=2.0, kill_timeout=2.0):
    """Clean a caller-owned new-session process; report persistent failures."""
    cleanup_process_group(proc, term_timeout=term_timeout, kill_timeout=kill_timeout)


def run_captured(argv, *, cwd=None, env=None, timeout):
    """Capture one bounded command and clean its private group on every exit.

    A command's timeout/interruption remains the primary exception if cleanup
    also fails. The cleanup error is attached as ``cleanup_error`` and a note.
    A failed completed command plus failed cleanup raises CalledProcessError
    carrying its actual exit status/output and the same cleanup detail.
    """
    with interruptible():
        proc = subprocess.Popen(argv, cwd=cwd, env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, start_new_session=True)
        primary = None
        stdout = stderr = None
        try:
            try:
                stdout, stderr = proc.communicate(timeout=timeout)
            except BaseException as exc:
                primary = exc
            try:
                terminate_group(proc)
            except BaseException as cleanup_error:
                if primary is None and proc.returncode not in (None, 0):
                    primary = subprocess.CalledProcessError(proc.returncode, argv,
                                                            output=stdout, stderr=stderr)
                if primary is None:
                    raise
                primary.cleanup_error = cleanup_error
                primary.add_note(f"process-group cleanup also failed: {cleanup_error}")
            if primary is not None:
                raise primary
            return subprocess.CompletedProcess(argv, proc.returncode, stdout, stderr)
        finally:
            # Do not use Popen's context manager: its unbounded wait could hide
            # a real cleanup failure. The group cleanup already polls/reaps the
            # leader, and any still-live leader is explicitly reported.
            proc.stdout.close()
            proc.stderr.close()
