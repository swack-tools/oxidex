#!/usr/bin/env python3
"""Tests for `doctor.check_ntp_offset`/`doctor.query_ntp_offset` (PLAN
Stage 3 task 7: "an NTP-offset check that REFUSES (non-zero) when the
host clock is more than 5 s off -- leases are absolute timestamps; a
skewed clock silently expires or over-extends them").

`check_ntp_offset` takes an injectable `query_fn` specifically so this
suite never depends on network access or on the real wall clock's
relationship to true time -- `query_ntp_offset` itself (the real SNTP
client) is exercised separately, against a hand-built in-process UDP
server standing in for a real NTP server, so the wire-format parsing is
still proven without a live network dependency.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import socket
import struct
import sys
import threading
import time
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import doctor  # noqa: E402
from _env import HermeticCase  # noqa: E402


# --------------------------------------------------------------------- #
# check_ntp_offset: pure logic, injected query_fn -- no network
# --------------------------------------------------------------------- #


class TestCheckNtpOffset(HermeticCase):
    def test_offset_within_floor_passes(self):
        c = doctor.check_ntp_offset(query_fn=lambda: 0.0)
        self.assertTrue(c.ok, msg=c.detail)
        self.assertEqual(c.value, 0.0)

    def test_offset_just_inside_the_floor_passes(self):
        c = doctor.check_ntp_offset(query_fn=lambda: doctor.MAX_CLOCK_SKEW_S - 0.001)
        self.assertTrue(c.ok, msg=c.detail)

    def test_offset_over_the_positive_floor_refuses(self):
        """Fast clock: a lease this host thinks is still fresh has
        already timed out from the store's point of view."""
        c = doctor.check_ntp_offset(query_fn=lambda: doctor.MAX_CLOCK_SKEW_S + 0.5)
        self.assertFalse(c.ok, msg="a clock 5.5s fast must FAIL, not warn")
        self.assertIn("exceeds", c.detail)
        self.assertIn(str(doctor.MAX_CLOCK_SKEW_S), c.detail)

    def test_offset_over_the_negative_floor_refuses(self):
        """Slow clock is symmetric: over-extends a lease from the
        store's point of view, exactly as dangerous as under-extending
        one, and must refuse just the same."""
        c = doctor.check_ntp_offset(query_fn=lambda: -(doctor.MAX_CLOCK_SKEW_S + 0.5))
        self.assertFalse(c.ok, msg="a clock 5.5s slow must FAIL, not warn")

    def test_offset_exactly_at_the_floor_passes(self):
        """`> MAX_CLOCK_SKEW_S`, not `>=` -- exactly 5.000s is still
        within the floor, per the check's own `abs(offset) >
        MAX_CLOCK_SKEW_S` condition."""
        c = doctor.check_ntp_offset(query_fn=lambda: doctor.MAX_CLOCK_SKEW_S)
        self.assertTrue(c.ok, msg=c.detail)

    def test_an_unmeasurable_clock_is_a_failure_not_a_skip(self):
        """Module-wide rule (doctor.py's own docstring): a check this
        script cannot perform is a FAIL, never a silent skip -- a host
        with no network route to any NTP server must not pass by
        default."""

        def boom():
            raise OSError("network is unreachable")

        c = doctor.check_ntp_offset(query_fn=boom)
        self.assertFalse(c.ok)
        self.assertFalse(c.informational, "must not be downgraded to INFO")
        self.assertIn("unreachable", c.detail)

    def test_timeout_is_also_a_failure(self):
        """`socket.timeout` is an `OSError` subclass -- confirm the
        except clause actually catches the shape a real timed-out UDP
        read raises, not just a bare OSError built by hand."""

        def boom():
            raise socket.timeout("timed out")

        c = doctor.check_ntp_offset(query_fn=boom)
        self.assertFalse(c.ok)

    def test_check_is_included_in_main_checks(self):
        """Guard against the function being added but never wired into
        `main()`'s checks list -- the analogous fence
        `test_doctor_git_token_file.py` uses for its own (argument-less)
        check, adapted for `check_ntp_offset`'s injectable `query_fn`
        parameter: the DEFINITION carries the parameter, but the call
        inside `main()`'s checks list must be the bare, default-argument
        form -- that IS the production call, real network query and
        all."""
        source = (FLEET_DIR / "doctor.py").read_text()
        self.assertIn("def check_ntp_offset(", source)
        self.assertIn("check_ntp_offset(),", source, "main()'s checks list must call it with no arguments")

    def test_check_name_is_stable(self):
        """`registration_payload` looks this check up by name; a rename
        here without updating there would silently drop `ntp_offset_s`
        from the JSON payload with no error anywhere."""
        c = doctor.check_ntp_offset(query_fn=lambda: 0.0)
        self.assertEqual(c.name, "clock (NTP offset)")


# --------------------------------------------------------------------- #
# query_ntp_offset: the real SNTP client, against a fake UDP server
# --------------------------------------------------------------------- #


_NTP_EPOCH_DELTA = 2208988800


def _fake_ntp_server(offset_s: float, stop_event: threading.Event):
    """A minimal UDP server that answers exactly one SNTP request with a
    reply implying the server's clock is `offset_s` seconds ahead of
    whatever `time.time()` says right now -- close enough for this
    test's purpose (the reply's own two timestamps are what
    `query_ntp_offset` actually reads; the request's origin timestamp is
    not echoed back, which is fine since `query_ntp_offset` does not
    read it either). Returns the bound port.
    """
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("127.0.0.1", 0))
    sock.settimeout(2.0)
    port = sock.getsockname()[1]

    def serve():
        try:
            _data, addr = sock.recvfrom(48)
        except OSError:
            return
        server_time = time.time() + offset_s
        seconds = int(server_time) + _NTP_EPOCH_DELTA
        frac = int((server_time % 1) * (2**32))
        reply = bytearray(48)
        reply[0] = 0x24  # LI=0, VN=4, Mode=4 (server)
        reply[32:40] = struct.pack("!II", seconds, frac)  # Receive Timestamp
        reply[40:48] = struct.pack("!II", seconds, frac)  # Transmit Timestamp
        try:
            sock.sendto(bytes(reply), addr)
        except OSError:
            pass
        finally:
            sock.close()

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    return port, thread


class TestQueryNtpOffsetWireFormat(HermeticCase):
    """Exercises the real SNTP client end to end against a fake, local
    (127.0.0.1-only) server -- proves the RFC 4330 byte layout is parsed
    correctly, without depending on the network or on any real time
    server being reachable from wherever this suite runs."""

    def test_offset_is_recovered_from_a_fake_server(self):
        port, thread = _fake_ntp_server(3.0, threading.Event())
        try:
            offset = doctor.query_ntp_offset(server="127.0.0.1", port=port, timeout=2.0)
        finally:
            thread.join(timeout=2.0)
        # Generous tolerance: this test's own scheduling jitter (thread
        # start, GIL, CI noise) is on the order of tens of milliseconds,
        # nowhere near doctor.MAX_CLOCK_SKEW_S -- the assertion is "the
        # SNTP arithmetic recovered the injected 3s offset", not "this
        # machine's scheduler is real-time".
        self.assertAlmostEqual(offset, 3.0, delta=0.5)

    def test_negative_offset_is_recovered(self):
        port, thread = _fake_ntp_server(-2.5, threading.Event())
        try:
            offset = doctor.query_ntp_offset(server="127.0.0.1", port=port, timeout=2.0)
        finally:
            thread.join(timeout=2.0)
        self.assertAlmostEqual(offset, -2.5, delta=0.5)

    def test_no_server_listening_raises_os_error(self):
        """A closed UDP port on loopback: `connection refused` (ICMP
        port-unreachable) or a timeout, both `OSError` -- either way,
        `query_ntp_offset` must raise rather than return a fabricated
        number."""
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
        sock.close()  # now definitely nothing is listening
        with self.assertRaises(OSError):
            doctor.query_ntp_offset(server="127.0.0.1", port=port, timeout=1.0)

    def test_short_reply_raises_os_error(self):
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind(("127.0.0.1", 0))
        sock.settimeout(2.0)
        port = sock.getsockname()[1]

        def serve():
            try:
                _data, addr = sock.recvfrom(48)
                sock.sendto(b"\x00" * 4, addr)  # far short of 48 bytes
            except OSError:
                pass
            finally:
                sock.close()

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        try:
            with self.assertRaises(OSError):
                doctor.query_ntp_offset(server="127.0.0.1", port=port, timeout=2.0)
        finally:
            thread.join(timeout=2.0)


if __name__ == "__main__":
    unittest.main()
