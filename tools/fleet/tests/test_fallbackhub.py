#!/usr/bin/env python3
"""Tests for tools/fleet/keel/fallbackhub.py -- SPEC 4.3's two rules,
driven through a FAULT-INJECTING fake primary.

THE FIXTURE. One `git init --bare` state repo under the system temp dir
stands in for `swack-tools/oxidex-fleet-state`. Two `fleetlib.Hub`
instances point at it with separate object caches: `self.github` (the
GitHub half of the FallbackHub under test, wrapped to count its writes)
and `self.store` (the fake primary's own handle -- the real `keel-server`
executes the identical `Hub.create/update/delete` against the state repo,
SPEC 4.2, and the fake does the same). `FakePrimary` implements the Hub
method surface over `self.store` and injects faults BEFORE or AFTER the
CAS executes, which is the whole distinction r2 turns on:

  * before-send faults (connection refused, DNS, TLS, `503 not-ready`):
    the store is untouched and the FallbackHub must re-issue against
    GitHub and return GitHub's answer;
  * after-CAS faults (timeout after send, dropped connection, 5xx after
    execution, an unclassifiable `HubUnreachableError`): the store shows
    exactly ONE write, the FallbackHub must RAISE `HubUnreachableError`,
    and the GitHub half must never be asked to write.

r1 (fresh claims) is the primary's rule, not FallbackHub's, but the
route-flip test here drives claim.py's real `Claim.renew()` through a
FallbackHub -- server, then direct while the server is "dead", then the
restarted server with a deliberately stale index -- and asserts `lost`
stays False. Its negative control flips the fake primary to serve the
claim sha from that stale index and asserts the lease goes red, so the
test is known to detect the violation it exists for.

EVERY NEW TEST FAILED WITH ITS BUG PRESENT. Checked by mutation from a
scratch runner, never by editing the module: patching
`fallbackhub.classify_primary_failure` to always answer BEFORE_SEND (a
FallbackHub that retries ambiguous writes) fails every
`TestAmbiguousWriteRaises` case; patching it to always answer AMBIGUOUS
fails every `TestBeforeSendFallsBack` write case; setting `sticky_s=0`
fails `TestSticky`; `claims_fresh=False` is the in-suite negative
control for the route flip. Hermetic: no network, no `exiftool`, no
production hub (`_assert_not_production` refuses the hub host and the
code repo by name before any git command runs).

    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_fallbackhub -v
"""

from __future__ import annotations

import http.client
import os
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # tools/fleet

from _env import HermeticCase  # noqa: E402
from claim import Claim  # noqa: E402
from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402
from keel import fallbackhub  # noqa: E402
from keel.fallbackhub import (  # noqa: E402
    AMBIGUOUS,
    BEFORE_SEND,
    AmbiguousWriteError,
    FallbackHub,
    PrimaryFailure,
    classify_primary_failure,
)

_FORBIDDEN_HUB_SUBSTRINGS = ("work2.oxidex.net",)
_FORBIDDEN_HUB_SUFFIXES = ("/oxidex.git", "/oxidex")


def _assert_not_production(url: str) -> None:
    low = url.lower()
    for needle in _FORBIDDEN_HUB_SUBSTRINGS:
        if needle in low:
            raise AssertionError(f"refusing to run against the production hub: {url}")
    for suffix in _FORBIDDEN_HUB_SUFFIXES:
        if low.rstrip("/").endswith(suffix):
            raise AssertionError(f"refusing to run against the code repo: {url}")


# --------------------------------------------------------------------- #
# Test doubles
# --------------------------------------------------------------------- #


class FakeClock:
    """Monotonic seconds under test control (FallbackHub's `clock=`)."""

    def __init__(self, start: float = 1000.0):
        self.t = float(start)

    def __call__(self) -> float:
        return self.t

    def advance(self, seconds: float) -> None:
        self.t += float(seconds)


class CountingHub(Hub):
    """The GitHub half: a real `fleetlib.Hub` that counts its CAS writes,
    so "the github store shows exactly ONE write" is a number and not an
    inference."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.writes = 0
        self.write_log: list = []

    def create(self, ref, payload, push_options=None):
        self.writes += 1
        self.write_log.append(("create", ref))
        return super().create(ref, payload, push_options=push_options)

    def update(self, ref, payload, expect_sha, push_options=None):
        self.writes += 1
        self.write_log.append(("update", ref))
        return super().update(ref, payload, expect_sha, push_options=push_options)

    def delete(self, ref, expect_sha, push_options=None):
        self.writes += 1
        self.write_log.append(("delete", ref))
        return super().delete(ref, expect_sha, push_options=push_options)


def _refused() -> HubUnreachableError:
    exc = HubUnreachableError("connect to primary: [Errno 61] Connection refused")
    exc.__cause__ = ConnectionRefusedError(61, "Connection refused")
    return exc


def _dns() -> HubUnreachableError:
    exc = HubUnreachableError("connect to primary: name resolution failed")
    exc.__cause__ = urllib.error.URLError(socket.gaierror(8, "nodename nor servname provided"))
    return exc


def _not_ready_httperror() -> HubUnreachableError:
    exc = HubUnreachableError("primary answered 503 not-ready")
    exc.__cause__ = urllib.error.HTTPError("http://primary/v1/refs/x", 503, "not ready", {}, None)
    return exc


def _timeout() -> HubUnreachableError:
    exc = HubUnreachableError("primary: read timed out after 20 s")
    exc.__cause__ = socket.timeout("timed out")
    return exc


def _dropped() -> HubUnreachableError:
    exc = HubUnreachableError("primary: connection dropped")
    exc.__cause__ = http.client.RemoteDisconnected("Remote end closed connection without response")
    return exc


def _five_hundred() -> HubUnreachableError:
    exc = HubUnreachableError("primary answered 500")
    exc.__cause__ = urllib.error.HTTPError("http://primary/v1/refs/x", 500, "boom", {}, None)
    return exc


# fault name -> (phase, raise-before-CAS?, factory). "before" faults never
# touch the store; "after" faults execute the CAS first and then raise.
_FAULTS = {
    "refuse": ("before", _refused),
    "refuse-raw": ("before", lambda: ConnectionRefusedError(61, "Connection refused")),
    "dns": ("before", _dns),
    "not-ready": ("before", lambda: PrimaryFailure("503 not-ready (settle)", request_sent=False, status=503)),
    "not-ready-httperror": ("before", _not_ready_httperror),
    "timeout-no-exec": ("before", _timeout),  # timed out, server never executed: STILL ambiguous
    "timeout-after-cas": ("after", _timeout),
    "dropped-after-cas": ("after", _dropped),
    "500-after-cas": ("after", _five_hundred),
    "bare-after-cas": ("after", lambda: HubUnreachableError("primary failed (no detail)")),
}


class FakePrimary:
    """A `ServerHub` stand-in with the Hub method surface over `store`.

    `fault` names an entry of `_FAULTS` (or None). `calls` records every
    method invocation as `(op, ref)`; `write_calls` counts CAS attempts
    that reached the store; `executed` holds the sha each executed CAS
    left on the ref, so a test can assert the store's ref IS that sha
    (one write) rather than merely "changed".

    r1 knob: `claims_fresh` (default True) answers claim-namespace reads
    LIVE from the store; False answers them from `index`, the stale
    snapshot `kill()` takes -- the violation SPEC 4.3 r1 forbids.
    """

    def __init__(self, store: Hub):
        self.store = store
        self.fault: str | None = None
        # Which ops the fault applies to; None = every op. A primary whose
        # reads are index-served and fine while the GitHub push behind its
        # writes is slow is `fault_ops={"update"}` -- Seam 11's shape.
        self.fault_ops: set | None = None
        self.calls: list = []
        self.write_calls = 0
        self.executed: list = []
        self.claims_fresh = True
        self.index: dict = {}

    # -- lifecycle knobs ------------------------------------------------ #

    def kill(self) -> None:
        """Die with the index as it stands: every later call is refused."""
        self.index = {
            ref: self.store.read_with_sha(ref)
            for ref in self.store.fetch_namespace("refs/fleet")
        }
        self.fault = "refuse"

    def restart(self) -> None:
        """Come back WITHOUT re-sweeping: the index is what it was at kill."""
        self.fault = None

    # -- fault plumbing --------------------------------------------------- #

    def _armed(self, op: str) -> bool:
        return self.fault is not None and (self.fault_ops is None or op in self.fault_ops)

    def _before(self, op: str, ref: str) -> None:
        self.calls.append((op, ref))
        self._current_op = op
        if not self._armed(op):
            return
        phase, factory = _FAULTS[self.fault]
        if phase == "before":
            raise factory()

    def _after(self) -> None:
        if not self._armed(self._current_op):
            return
        phase, factory = _FAULTS[self.fault]
        if phase == "after":
            raise factory()

    def _is_claim(self, ref: str) -> bool:
        return ref.startswith(fallbackhub.CLAIMS_PREFIX)

    # -- reads ---------------------------------------------------------- #

    def sha(self, ref):
        self._before("sha", ref)
        if self._is_claim(ref) and not self.claims_fresh and ref in self.index:
            out = self.index[ref][0]
        else:
            out = self.store.sha(ref)
        self._after()
        return out

    def read(self, ref):
        self._before("read", ref)
        if self._is_claim(ref) and not self.claims_fresh and ref in self.index:
            out = self.index[ref][1]
        else:
            out = self.store.read(ref)
        self._after()
        return out

    def read_with_sha(self, ref):
        self._before("read_with_sha", ref)
        if self._is_claim(ref) and not self.claims_fresh and ref in self.index:
            out = self.index[ref]
        else:
            out = self.store.read_with_sha(ref)
        self._after()
        return out

    def list(self, prefix):
        self._before("list", prefix)
        out = self.store.list(prefix)
        self._after()
        return out

    def fetch_namespace(self, prefix):
        self._before("fetch_namespace", prefix)
        out = self.store.fetch_namespace(prefix)
        self._after()
        return out

    # -- writes --------------------------------------------------------- #

    def create(self, ref, payload, push_options=None):
        self._before("create", ref)
        self.write_calls += 1
        ok = self.store.create(ref, payload)
        if ok:
            self.executed.append(self.store.sha(ref))
        self._after()
        return ok

    def update(self, ref, payload, expect_sha, push_options=None):
        self._before("update", ref)
        self.write_calls += 1
        ok = self.store.update(ref, payload, expect_sha)
        if ok:
            self.executed.append(self.store.sha(ref))
        self._after()
        return ok

    def delete(self, ref, expect_sha, push_options=None):
        self._before("delete", ref)
        self.write_calls += 1
        ok = self.store.delete(ref, expect_sha)
        if ok:
            self.executed.append(None)
        self._after()
        return ok

    def push_ref(self, *args, **kwargs):
        raise NotImplementedError("branch pushes never go through the server")


class NoBulkReadPrimary(FakePrimary):
    """A primary without `fetch_namespace` (an older ServerHub)."""
    fetch_namespace = None  # type: ignore[assignment]


# --------------------------------------------------------------------- #
# Fixture
# --------------------------------------------------------------------- #


class FallbackHubTestCase(HermeticCase):
    def setUp(self):
        super().setUp()
        self.tmp = Path(tempfile.mkdtemp(prefix="fallbackhub-"))
        self.assertTrue(
            str(self.tmp).startswith(str(Path(tempfile.gettempdir()).resolve()))
            or str(self.tmp.resolve()).startswith(str(Path(tempfile.gettempdir()).resolve())),
            f"fixture repo must live under the temp dir: {self.tmp}",
        )
        self.state = self.tmp / "state.git"
        subprocess.run(["git", "init", "--quiet", "--bare", str(self.state)], check=True)
        _assert_not_production(str(self.state))
        self.github = CountingHub(url=str(self.state), workdir=self.tmp / "github-cache")
        self.store = Hub(url=str(self.state), workdir=self.tmp / "server-cache")
        self.primary = FakePrimary(self.store)
        self.clock = FakeClock()
        self.fb = FallbackHub(self.primary, self.github, clock=self.clock)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def ref(self, name: str) -> str:
        return f"refs/fleet/test/{name}"

    def claim_ref(self, name: str) -> str:
        return f"refs/fleet/claims/gate/{name}"

    def primary_ops(self) -> list:
        return [op for op, _ in self.primary.calls]


# --------------------------------------------------------------------- #
# classify_primary_failure: the before-send / ambiguous vocabulary
# --------------------------------------------------------------------- #


class TestClassification(HermeticCase):
    def test_connection_refused_is_before_send(self):
        self.assertEqual(classify_primary_failure(_refused()), BEFORE_SEND)
        self.assertEqual(classify_primary_failure(ConnectionRefusedError(61, "refused")), BEFORE_SEND)

    def test_no_route_errnos_are_before_send(self):
        import errno
        for code in (errno.EHOSTUNREACH, errno.ENETUNREACH, errno.ECONNREFUSED):
            with self.subTest(errno=code):
                self.assertEqual(classify_primary_failure(OSError(code, os.strerror(code))), BEFORE_SEND)

    def test_dns_is_before_send(self):
        self.assertEqual(classify_primary_failure(_dns()), BEFORE_SEND)
        self.assertEqual(classify_primary_failure(socket.gaierror(8, "nodename")), BEFORE_SEND)

    def test_tls_handshake_is_before_send(self):
        self.assertEqual(classify_primary_failure(ssl.SSLCertVerificationError("bad cert")), BEFORE_SEND)
        exc = ssl.SSLError(1, "[SSL: SSLV3_ALERT_HANDSHAKE_FAILURE] handshake failure")
        self.assertEqual(classify_primary_failure(exc), BEFORE_SEND)

    def test_503_is_before_send(self):
        self.assertEqual(classify_primary_failure(_not_ready_httperror()), BEFORE_SEND)
        self.assertEqual(classify_primary_failure(PrimaryFailure("not ready", status=503)), BEFORE_SEND)

    def test_explicit_request_sent_false_is_before_send(self):
        exc = PrimaryFailure("x", request_sent=False)
        exc.__cause__ = socket.timeout("timed out")  # would be ambiguous by cause
        self.assertEqual(classify_primary_failure(exc), BEFORE_SEND)

    def test_explicit_request_sent_true_wins_over_a_refused_cause(self):
        exc = PrimaryFailure("x", request_sent=True)
        exc.__cause__ = ConnectionRefusedError(61, "refused")
        self.assertEqual(classify_primary_failure(exc), AMBIGUOUS)

    def test_timeouts_are_ambiguous(self):
        self.assertEqual(classify_primary_failure(_timeout()), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(TimeoutError("timed out")), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(urllib.error.URLError(socket.timeout("t"))), AMBIGUOUS)

    def test_dropped_connections_are_ambiguous(self):
        self.assertEqual(classify_primary_failure(_dropped()), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(ConnectionResetError(54, "reset")), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(BrokenPipeError(32, "pipe")), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(http.client.IncompleteRead(b"")), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(http.client.BadStatusLine("")), AMBIGUOUS)

    def test_other_5xx_are_ambiguous(self):
        for code in (500, 502, 504):
            with self.subTest(status=code):
                exc = urllib.error.HTTPError("http://p/x", code, "err", {}, None)
                self.assertEqual(classify_primary_failure(exc), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(_five_hundred()), AMBIGUOUS)

    def test_bare_unreachable_is_ambiguous_fail_closed(self):
        self.assertEqual(classify_primary_failure(HubUnreachableError("no detail")), AMBIGUOUS)
        self.assertEqual(classify_primary_failure(PrimaryFailure("no detail")), AMBIGUOUS)

    def test_non_handshake_ssl_error_is_ambiguous(self):
        self.assertEqual(classify_primary_failure(ssl.SSLEOFError("EOF in violation of protocol")), AMBIGUOUS)

    def test_cause_chain_is_cycle_safe(self):
        a = HubUnreachableError("a")
        b = HubUnreachableError("b")
        a.__cause__ = b
        b.__cause__ = a
        self.assertEqual(classify_primary_failure(a), AMBIGUOUS)


# --------------------------------------------------------------------- #
# Identity and code routing: the GitHub half's, never the server's
# --------------------------------------------------------------------- #


class TestIdentityAndCodeRouting(FallbackHubTestCase):
    def test_urls_and_workdir_are_the_github_halfs(self):
        gh = CountingHub(
            url=str(self.state),
            workdir=self.tmp / "gh2",
            code_url="https://example.invalid/code.git",
            code_push_url="https://example.invalid/code-push.git",
            tip_push_url="git@example.invalid:tip.git",
        )
        fb = FallbackHub(self.primary, gh, clock=self.clock)
        self.assertEqual(fb.url, gh.url)
        self.assertEqual(fb.workdir, gh.workdir)
        self.assertEqual(fb.code_url, "https://example.invalid/code.git")
        self.assertEqual(fb.code_push_url, "https://example.invalid/code-push.git")
        self.assertEqual(fb.tip_push_url, "git@example.invalid:tip.git")
        self.assertIs(fb.fallback, gh)
        self.assertIs(fb.github, gh)

    def test_constructor_refuses_a_missing_half(self):
        with self.assertRaises(ValueError):
            FallbackHub(None, self.github)
        with self.assertRaises(ValueError):
            FallbackHub(self.primary, None)

    def test_code_reads_and_writes_never_touch_the_primary(self):
        sha = self.github._write_commit({"x": 1})
        self.assertIsNone(self.fb.code_sha("refs/heads/nope"))
        self.assertEqual(self.fb.code_list("refs/heads"), {})
        res = self.fb.push_ref(f"{sha}:refs/heads/t")
        self.assertEqual(res.returncode, 0, res.describe())
        self.assertEqual(self.fb.code_sha("refs/heads/t"), sha)
        self.assertEqual(self.fb.code_list("refs/heads"), {"refs/heads/t": sha})
        res = self.fb.push_code_ref(f"{sha}:refs/heads/u")
        self.assertEqual(res.returncode, 0, res.describe())
        self.assertTrue(self.fb.delete_code_ref("refs/heads/u", sha))
        self.assertTrue(self.fb.delete_code_ref("refs/heads/t", sha))
        self.assertEqual(self.primary.calls, [])
        # ... even while the primary is DOWN: code never waits on the server.
        self.primary.fault = "refuse"
        self.assertIsNone(self.fb.code_sha("refs/heads/t"))
        self.assertEqual(self.primary.calls, [])
        self.assertIsNone(self.fb.degraded_since)


# --------------------------------------------------------------------- #
# Healthy primary: everything routes to it, GitHub half idle
# --------------------------------------------------------------------- #


class TestHealthyPrimary(FallbackHubTestCase):
    def test_writes_and_reads_go_via_the_primary(self):
        ref = self.ref("one")
        self.assertTrue(self.fb.create(ref, {"n": 1}))
        sha1 = self.fb.sha(ref)
        self.assertEqual(sha1, self.store.sha(ref))
        self.assertTrue(self.fb.update(ref, {"n": 2}, sha1))
        sha2, payload = self.fb.read_with_sha(ref)
        self.assertEqual(payload["n"], 2)
        self.assertEqual(self.fb.read(ref)["n"], 2)
        self.assertEqual(self.fb.list("refs/fleet/test"), {ref: sha2})
        self.assertEqual(self.fb.fetch_namespace("refs/fleet/test"), {ref: sha2})
        self.assertTrue(self.fb.delete(ref, sha2))
        self.assertIsNone(self.fb.sha(ref))
        self.assertEqual(self.github.writes, 0)
        self.assertEqual(self.primary.write_calls, 3)
        self.assertIsNone(self.fb.degraded_since)
        self.assertFalse(self.fb.degraded)
        self.assertEqual(self.fb.status()["route"], "primary")
        self.assertEqual(self.fb.status()["primary_failures"], 0)

    def test_lost_race_false_is_passed_through_not_retried(self):
        ref = self.ref("race")
        self.assertTrue(self.fb.create(ref, {"n": 1}))
        self.assertFalse(self.fb.create(ref, {"n": 1}))  # exists: a lost race, not a failure
        stale = "0" * 40
        self.assertFalse(self.fb.update(ref, {"n": 2}, stale))
        self.assertFalse(self.fb.delete(ref, stale))
        self.assertEqual(self.github.writes, 0)
        self.assertIsNone(self.fb.degraded_since)

    def test_content_errors_are_not_route_failures(self):
        # A HubError that is NOT unreachable is a fact about the store;
        # switching routes would not change it, so it propagates as-is
        # and the primary is not marked degraded.
        class ContentErrorPrimary(FakePrimary):
            def read(self, ref):
                raise HubError(f"{ref}@deadbeef payload.json is not valid JSON")

        fb = FallbackHub(ContentErrorPrimary(self.store), self.github, clock=self.clock)
        with self.assertRaises(HubError) as cm:
            fb.read(self.ref("bad"))
        self.assertNotIsInstance(cm.exception, HubUnreachableError)
        self.assertIsNone(fb.degraded_since)

    def test_push_options_are_forwarded_only_when_given(self):
        seen = []

        class RecordingPrimary(FakePrimary):
            def create(self, ref, payload, **kw):
                seen.append(kw)
                return super().create(ref, payload, **kw)

        fb = FallbackHub(RecordingPrimary(self.store), self.github, clock=self.clock)
        self.assertTrue(fb.create(self.ref("a"), {"n": 1}))
        self.assertTrue(fb.create(self.ref("b"), {"n": 1}, push_options=["train-token=x"]))
        self.assertEqual(seen, [{}, {"push_options": ["train-token=x"]}])


# --------------------------------------------------------------------- #
# Before-send failures: fall back and return GitHub's result
# --------------------------------------------------------------------- #


class TestBeforeSendFallsBack(FallbackHubTestCase):
    def _assert_fell_back_for_write(self, fault: str):
        ref = self.ref(f"w-{fault}")
        self.assertTrue(self.github.create(ref, {"n": 0}))
        sha0 = self.github.sha(ref)
        writes_before = self.github.writes
        self.primary.fault = fault
        self.assertTrue(self.fb.update(ref, {"n": 1}, sha0), fault)
        self.assertEqual(self.github.writes, writes_before + 1, fault)
        self.assertEqual(self.primary.write_calls, 0, fault)
        self.assertEqual(self.store.read(ref)["n"], 1, fault)
        self.assertIsNotNone(self.fb.degraded_since, fault)
        self.assertEqual(self.fb.status()["route"], "github")

    def test_connection_refused_falls_back_on_update(self):
        self._assert_fell_back_for_write("refuse")

    def test_raw_connection_refused_falls_back_on_update(self):
        self._assert_fell_back_for_write("refuse-raw")

    def test_dns_failure_falls_back_on_update(self):
        self._assert_fell_back_for_write("dns")

    def test_503_not_ready_falls_back_on_update(self):
        self._assert_fell_back_for_write("not-ready")

    def test_503_httperror_falls_back_on_update(self):
        self._assert_fell_back_for_write("not-ready-httperror")

    def test_refused_create_returns_githubs_result(self):
        ref = self.ref("c")
        self.primary.fault = "refuse"
        self.assertTrue(self.fb.create(ref, {"n": 1}))
        self.assertEqual(self.github.writes, 1)
        self.assertEqual(self.store.read(ref)["n"], 1)
        # And GitHub's False is GitHub's answer, passed through.
        self.assertFalse(self.fb.create(ref, {"n": 1}))
        self.assertEqual(self.primary.write_calls, 0)

    def test_refused_delete_returns_githubs_result(self):
        ref = self.ref("d")
        self.assertTrue(self.github.create(ref, {"n": 1}))
        sha = self.github.sha(ref)
        self.primary.fault = "refuse"
        self.assertTrue(self.fb.delete(ref, sha))
        self.assertIsNone(self.store.sha(ref))
        self.assertFalse(self.fb.delete(ref, sha))  # already gone: a lost race
        self.assertEqual(self.primary.write_calls, 0)

    def test_reads_fall_back_on_refuse(self):
        ref = self.ref("r")
        self.assertTrue(self.github.create(ref, {"n": 7}))
        sha = self.github.sha(ref)
        self.primary.fault = "refuse"
        self.assertEqual(self.fb.sha(ref), sha)
        self.assertEqual(self.fb.read(ref)["n"], 7)
        self.assertEqual(self.fb.read_with_sha(ref), (sha, self.store.read(ref)))
        self.assertEqual(self.fb.list("refs/fleet/test"), {ref: sha})
        self.assertEqual(self.fb.fetch_namespace("refs/fleet/test"), {ref: sha})
        self.assertEqual(self.fb.status()["fallback_reads"], 5)

    def test_reads_fall_back_on_AMBIGUOUS_failures_too(self):
        # Reads have no side effect to duplicate: a timeout after send on
        # a GET is answered from GitHub, never raised.
        ref = self.ref("ra")
        self.assertTrue(self.github.create(ref, {"n": 9}))
        sha = self.github.sha(ref)
        for fault in ("timeout-no-exec", "timeout-after-cas", "dropped-after-cas", "500-after-cas", "bare-after-cas"):
            with self.subTest(fault=fault):
                self.primary.fault = fault
                self.clock.advance(fallbackhub.STICKY_S + 1)  # leave any sticky window
                self.assertEqual(self.fb.sha(ref), sha)
                self.clock.advance(fallbackhub.STICKY_S + 1)
                self.assertEqual(self.fb.read(ref)["n"], 9)
                self.clock.advance(fallbackhub.STICKY_S + 1)
                self.assertEqual(self.fb.read_with_sha(ref)[0], sha)
                self.clock.advance(fallbackhub.STICKY_S + 1)
                self.assertEqual(self.fb.list("refs/fleet/test"), {ref: sha})
        self.assertEqual(self.github.writes, 1)

    def test_fetch_namespace_without_a_primary_implementation_uses_github(self):
        ref = self.ref("ns")
        self.assertTrue(self.github.create(ref, {"n": 1}))
        primary = NoBulkReadPrimary(self.store)
        fb = FallbackHub(primary, self.github, clock=self.clock)
        self.assertEqual(fb.fetch_namespace("refs/fleet/test"), {ref: self.github.sha(ref)})
        self.assertEqual(primary.calls, [])
        self.assertIsNone(fb.degraded_since)

    def test_both_routes_down_raises_unreachable(self):
        gone = CountingHub(url=str(self.tmp / "does-not-exist.git"), workdir=self.tmp / "gone-cache")
        fb = FallbackHub(self.primary, gone, clock=self.clock)
        self.primary.fault = "refuse"
        with self.assertRaises(HubUnreachableError):
            fb.sha(self.ref("x"))
        with self.assertRaises(HubUnreachableError):
            fb.create(self.ref("x"), {"n": 1})
        self.assertEqual(self.primary.write_calls, 0)


# --------------------------------------------------------------------- #
# r2: ambiguous failures RAISE; the store shows exactly one write
# --------------------------------------------------------------------- #


class TestAmbiguousWriteRaises(FallbackHubTestCase):
    def _assert_update_raises_with_one_write(self, fault: str):
        ref = self.ref(f"u-{fault}")
        self.assertTrue(self.github.create(ref, {"n": 0}))
        sha0 = self.github.sha(ref)
        writes_before = self.github.writes
        self.primary.fault = fault
        with self.assertRaises(HubUnreachableError) as cm:
            self.fb.update(ref, {"n": 1}, sha0)
        self.assertIsInstance(cm.exception, AmbiguousWriteError, fault)
        self.assertEqual(cm.exception.op, "update")
        self.assertEqual(cm.exception.ref, ref)
        # Exactly ONE write: the primary's CAS executed, the ref IS the
        # sha that CAS produced, and the GitHub half was never asked.
        self.assertEqual(self.primary.write_calls, 1, fault)
        self.assertEqual(self.github.writes, writes_before, fault)
        self.assertEqual(len(self.primary.executed), 1, fault)
        self.assertEqual(self.store.sha(ref), self.primary.executed[0], fault)
        self.assertNotEqual(self.store.sha(ref), sha0, fault)
        self.assertEqual(self.store.read(ref)["n"], 1, fault)
        self.assertEqual(self.fb.status()["ambiguous_writes"], 1)
        self.assertIsNotNone(self.fb.degraded_since)

    def test_timeout_after_cas_raises_never_false_never_second_write(self):
        self._assert_update_raises_with_one_write("timeout-after-cas")

    def test_dropped_connection_after_cas_raises(self):
        self._assert_update_raises_with_one_write("dropped-after-cas")

    def test_5xx_after_cas_raises(self):
        self._assert_update_raises_with_one_write("500-after-cas")

    def test_unclassifiable_failure_after_cas_raises(self):
        self._assert_update_raises_with_one_write("bare-after-cas")

    def test_ambiguous_create_raises_and_the_ref_exists_once(self):
        ref = self.ref("c-amb")
        self.primary.fault = "timeout-after-cas"
        with self.assertRaises(AmbiguousWriteError) as cm:
            self.fb.create(ref, {"n": 1})
        self.assertEqual(cm.exception.op, "create")
        self.assertEqual(self.github.writes, 0)
        self.assertEqual(self.primary.write_calls, 1)
        self.assertEqual(self.store.sha(ref), self.primary.executed[0])
        self.assertEqual(self.store.read(ref)["n"], 1)

    def test_ambiguous_delete_raises_and_github_is_not_asked(self):
        ref = self.ref("d-amb")
        self.assertTrue(self.github.create(ref, {"n": 1}))
        sha = self.github.sha(ref)
        writes_before = self.github.writes
        self.primary.fault = "dropped-after-cas"
        with self.assertRaises(AmbiguousWriteError) as cm:
            self.fb.delete(ref, sha)
        self.assertEqual(cm.exception.op, "delete")
        self.assertIsNone(self.store.sha(ref))  # the primary's delete landed
        self.assertEqual(self.github.writes, writes_before)
        self.assertEqual(self.primary.write_calls, 1)

    def test_timeout_with_no_execution_still_raises(self):
        # The caller cannot know the server did NOT execute; neither can we.
        ref = self.ref("t-noexec")
        self.assertTrue(self.github.create(ref, {"n": 0}))
        sha0 = self.github.sha(ref)
        writes_before = self.github.writes
        self.primary.fault = "timeout-no-exec"
        with self.assertRaises(AmbiguousWriteError):
            self.fb.update(ref, {"n": 1}, sha0)
        self.assertEqual(self.store.sha(ref), sha0)  # untouched
        self.assertEqual(self.store.read(ref)["n"], 0)
        self.assertEqual(self.github.writes, writes_before)
        self.assertEqual(self.primary.write_calls, 0)

    def test_ambiguous_error_is_a_hub_unreachable_error(self):
        # Every existing consumer (`claim._note_renew_failure`,
        # `verdict._cli_store`, fleetd's degraded-step counter) keys on
        # this class; the new error must be one.
        err = AmbiguousWriteError("update", "refs/fleet/x", socket.timeout("t"))
        self.assertIsInstance(err, HubUnreachableError)
        self.assertIsInstance(err, HubError)
        self.assertIn("NOT re-issued", str(err))

    def test_next_write_inside_the_sticky_window_goes_direct_not_to_the_primary(self):
        # After an ambiguous failure the primary is not contacted again
        # for 30 s -- but a DIRECT write in that window is not a retry of
        # the ambiguous one: it is a new CAS against the sha the caller
        # re-read, which is what claim.renew() does at its top of loop.
        ref = self.ref("after-amb")
        self.assertTrue(self.github.create(ref, {"n": 0}))
        sha0 = self.github.sha(ref)
        self.primary.fault = "timeout-after-cas"
        with self.assertRaises(AmbiguousWriteError):
            self.fb.update(ref, {"n": 1}, sha0)
        calls_after_failure = len(self.primary.calls)
        self.primary.fault = None  # it came back -- we do not know that yet
        self.clock.advance(5)
        landed = self.fb.sha(ref)  # served from GitHub: the primary's write is visible
        self.assertEqual(landed, self.primary.executed[0])
        self.assertTrue(self.fb.update(ref, {"n": 2}, landed))
        self.assertEqual(len(self.primary.calls), calls_after_failure)
        self.assertEqual(self.primary.write_calls, 1)
        self.assertEqual(self.store.read(ref)["n"], 2)

    def test_claim_renew_tolerates_the_ambiguous_blip_and_adopts_the_landed_write(self):
        # Seam 11 in miniature, through claim.py's REAL renew(): the
        # renewal's CAS lands on the store but the primary times out
        # afterwards. renew() returns False without marking lost
        # (`_note_renew_failure` tolerates one blip at a 600 s TTL); the
        # next renewal's re-read + `_owns` adopts the landed sha.
        claim = Claim(
            self.fb, "gate", "tree-amb", work_key="staging/amb",
            rustc_id="r", platform_id="p", ttl=600, renew_interval=120,
        )
        claim.acquire()
        claim.stop_renewer()
        try:
            self.primary.fault = "timeout-after-cas"
            self.primary.fault_ops = {"update"}  # reads answer; the write hangs after landing
            self.assertFalse(claim.renew())
            self.assertFalse(claim.lost, claim.lost_reason)
            self.assertEqual(self.primary.write_calls, 2)  # create + the landed update
            self.assertEqual(self.store.sha(claim.ref), self.primary.executed[-1])
            self.primary.fault = None
            self.clock.advance(fallbackhub.STICKY_S + 1)
            self.assertTrue(claim.renew())
            self.assertFalse(claim.lost, claim.lost_reason)
            self.assertEqual(self.github.writes, 0)  # every write went through the primary
        finally:
            claim.release()


# --------------------------------------------------------------------- #
# r3: sticky 30 s, then re-probe; degraded_since
# --------------------------------------------------------------------- #


class TestSticky(FallbackHubTestCase):
    def test_primary_is_not_contacted_for_sticky_s_after_a_failure(self):
        ref = self.ref("s")
        self.assertTrue(self.github.create(ref, {"n": 0}))
        self.primary.fault = "refuse"
        self.assertEqual(self.fb.read(ref)["n"], 0)
        frozen = len(self.primary.calls)
        self.assertEqual(frozen, 1)
        self.primary.fault = None  # back up, but we must not know for 30 s
        for dt in (1, 10, 18.9):
            self.clock.advance(dt)
            self.assertEqual(self.fb.read(ref)["n"], 0)
            self.assertIsNotNone(self.fb.sha(ref))
            self.assertTrue(self.fb.update(ref, {"n": 0}, self.fb.sha(ref)))
            self.assertEqual(len(self.primary.calls), frozen, f"primary contacted at +{self.clock.t - 1000:.1f}s")
        self.assertEqual(self.github.writes, 1 + 3)
        self.clock.advance(0.2)  # 30.1 s after the failure
        self.assertEqual(self.fb.read(ref)["n"], 0)
        self.assertEqual(len(self.primary.calls), frozen + 1)
        self.assertIsNone(self.fb.degraded_since)
        self.assertEqual(self.fb.status()["route"], "primary")

    def test_write_after_window_probes_with_a_read_then_routes_to_the_primary(self):
        ref = self.ref("probe")
        self.assertTrue(self.github.create(ref, {"n": 0}))
        self.primary.fault = "refuse"
        # Refused before send -> GitHub, whose answer is the lost race.
        self.assertFalse(self.fb.create(ref, {"n": 0}))
        self.assertIsNotNone(self.fb.degraded_since)
        self.primary.fault = None
        self.primary.calls.clear()
        self.clock.advance(fallbackhub.STICKY_S)
        sha = self.github.sha(ref)
        self.assertTrue(self.fb.update(ref, {"n": 1}, sha))
        self.assertEqual(self.primary.calls, [("sha", ref), ("update", ref)])
        self.assertEqual(self.primary.write_calls, 1)
        self.assertIsNone(self.fb.degraded_since)

    def test_failed_probe_keeps_the_write_direct_and_re_arms_the_window(self):
        ref = self.ref("reprobe")
        self.assertTrue(self.github.create(ref, {"n": 0}))
        self.primary.fault = "refuse"
        self.assertIsNotNone(self.fb.sha(ref))
        since = self.fb.degraded_since
        self.primary.calls.clear()
        self.clock.advance(fallbackhub.STICKY_S)
        self.assertTrue(self.fb.update(ref, {"n": 1}, self.github.sha(ref)))  # direct
        self.assertEqual(self.primary.calls, [("sha", ref)])  # the probe, refused
        self.assertEqual(self.primary.write_calls, 0)
        self.assertEqual(self.fb.degraded_since, since)  # "since" is the FIRST failure
        self.clock.advance(fallbackhub.STICKY_S / 2)
        self.assertTrue(self.fb.update(ref, {"n": 2}, self.github.sha(ref)))
        self.assertEqual(self.primary.calls, [("sha", ref)])  # re-armed: no contact
        self.clock.advance(fallbackhub.STICKY_S / 2)
        self.assertTrue(self.fb.update(ref, {"n": 3}, self.github.sha(ref)))
        self.assertEqual(self.primary.calls, [("sha", ref), ("sha", ref)])  # probed again
        self.assertEqual(self.github.writes, 4)
        self.assertEqual(self.fb.status()["fallback_writes"], 3)
        self.assertEqual(self.fb.status()["primary_failures"], 3)

    def test_injected_probe_is_used_instead_of_sha(self):
        probes = []
        fb = FallbackHub(self.primary, self.github, clock=self.clock, probe=lambda: probes.append(1))
        ref = self.ref("inj")
        self.primary.fault = "refuse"
        self.assertTrue(fb.create(ref, {"n": 0}))
        self.primary.fault = None
        self.primary.calls.clear()
        self.clock.advance(fallbackhub.STICKY_S)
        self.assertTrue(fb.update(ref, {"n": 1}, self.github.sha(ref)))
        self.assertEqual(probes, [1])
        self.assertEqual(self.primary.calls, [("update", ref)])

    def test_sticky_window_is_configurable(self):
        fb = FallbackHub(self.primary, self.github, clock=self.clock, sticky_s=5)
        ref = self.ref("cfg")
        self.primary.fault = "refuse"
        self.assertIsNone(fb.sha(ref))
        self.primary.fault = None
        n = len(self.primary.calls)
        self.clock.advance(4.9)
        self.assertIsNone(fb.sha(ref))
        self.assertEqual(len(self.primary.calls), n)
        self.clock.advance(0.1)
        self.assertIsNone(fb.sha(ref))
        self.assertEqual(len(self.primary.calls), n + 1)

    def test_degraded_since_is_utc_and_cleared_on_recovery(self):
        self.assertIsNone(self.fb.degraded_since)
        self.assertIsNone(self.fb.status()["degraded_since"])
        self.primary.fault = "refuse"
        self.fb.sha(self.ref("z"))
        since = self.fb.degraded_since
        self.assertIsInstance(since, datetime)
        self.assertEqual(since.tzinfo, timezone.utc)
        self.assertEqual(self.fb.status()["degraded_since"], since.isoformat())
        self.assertIn("Connection refused", self.fb.status()["last_primary_error"] or "")
        self.primary.fault = None
        self.clock.advance(fallbackhub.STICKY_S)
        self.fb.sha(self.ref("z"))
        self.assertIsNone(self.fb.degraded_since)
        self.assertFalse(self.fb.degraded)

    def test_default_clock_is_monotonic(self):
        fb = FallbackHub(self.primary, self.github)
        self.assertIs(fb._clock, time.monotonic)
        self.assertEqual(fb.sticky_s, fallbackhub.STICKY_S)
        self.assertEqual(fallbackhub.STICKY_S, 30.0)


# --------------------------------------------------------------------- #
# r1 through claim.py: a route flip never yields a stale sha
# --------------------------------------------------------------------- #


class TestRouteFlipNeverMarksLost(FallbackHubTestCase):
    """Seam 9 in miniature, against claim.py's real `renew()`."""

    def _claim(self, key: str = "tree-flip") -> Claim:
        return Claim(
            self.fb, "gate", key, work_key=f"staging/{key}",
            rustc_id="r", platform_id="p", ttl=600, renew_interval=120,
        )

    def _drive(self, claim: Claim) -> list:
        """Acquire via the primary, renew via the primary, kill the
        primary, renew DIRECT, restart the primary with its stale index,
        renew via the primary again. Returns the sha after each step."""
        shas = []
        claim.acquire()
        claim.stop_renewer()  # renewals are driven by hand below
        shas.append(self.store.sha(claim.ref))
        self.assertEqual(self.primary_ops()[:2], ["create", "sha"])

        self.assertTrue(claim.renew())
        self.assertFalse(claim.lost, claim.lost_reason)
        shas.append(self.store.sha(claim.ref))
        self.assertEqual(self.github.writes, 0)

        self.primary.kill()  # index snapshot = shas[-1]; every call refused
        self.assertTrue(claim.renew())  # direct
        self.assertFalse(claim.lost, claim.lost_reason)
        shas.append(self.store.sha(claim.ref))
        self.assertEqual(self.github.writes, 1)
        self.assertIsNotNone(self.fb.degraded_since)
        self.assertEqual(self.primary.index[claim.ref][0], shas[1])  # stale by one renewal

        self.primary.restart()
        self.primary.calls.clear()
        self.clock.advance(fallbackhub.STICKY_S + 1)
        ok = claim.renew()
        shas.append(self.store.sha(claim.ref))
        return [ok] + shas

    def test_route_flip_renews_via_the_restarted_primary_without_marking_lost(self):
        claim = self._claim()
        try:
            ok, s0, s1, s2, s3 = self._drive(claim)
            self.assertTrue(ok)
            self.assertFalse(claim.lost, claim.lost_reason)
            self.assertEqual(len({s0, s1, s2, s3}), 4)  # every renewal moved the ref
            # The last renewal went through the PRIMARY (probe, sha, update)
            # and its CAS witness was the live sha, not the stale one.
            self.assertIn("update", self.primary_ops())
            self.assertEqual(self.github.writes, 1)
            self.assertEqual(self.primary.executed[-1], s3)
            self.assertIsNone(self.fb.degraded_since)
            self.assertEqual(claim._sha, s3)
        finally:
            claim.release()

    def test_negative_control_index_served_claim_sha_marks_the_lease_lost(self):
        # A primary that answers the claim's sha from its stale index is
        # exactly the defect r1 forbids. claim.renew() re-reads the sha,
        # sees it differs, reads the (stale) payload, finds it ours,
        # ADOPTS the stale sha, and the store rejects the CAS -> lost.
        self.primary.claims_fresh = False
        claim = self._claim("tree-stale")
        try:
            ok, s0, s1, s2, s3 = self._drive(claim)
            self.assertFalse(ok)
            self.assertTrue(claim.lost)
            self.assertIn("rejected", claim.lost_reason)
            self.assertEqual(s3, s2)  # nothing landed
        finally:
            claim.release()

    def test_direct_renewal_then_sticky_window_keeps_the_lease(self):
        # Several renewals direct in a row, then recovery; lost never set.
        claim = self._claim("tree-long")
        try:
            claim.acquire()
            claim.stop_renewer()
            self.primary.kill()
            for _ in range(3):
                self.assertTrue(claim.renew())
                self.assertFalse(claim.lost, claim.lost_reason)
                self.clock.advance(120)
            self.primary.restart()
            self.assertTrue(claim.renew())
            self.assertFalse(claim.lost, claim.lost_reason)
            self.assertIsNone(self.fb.degraded_since)
            self.assertEqual(self.github.writes, 3)
        finally:
            claim.release()

    def test_adopt_continues_a_lease_renewed_on_the_other_route(self):
        # Consistency rule 3: the ownership token survives a route change,
        # so `Claim.adopt` (the restarted runner) continues a lease whose
        # last renewal went direct.
        claim = self._claim("tree-adopt")
        claim.acquire()
        claim.stop_renewer()
        self.primary.kill()
        self.assertTrue(claim.renew())
        host = claim.holder_host
        self.primary.restart()
        self.clock.advance(fallbackhub.STICKY_S + 1)
        adopted = Claim.adopt(self.fb, claim.ref, expected_host=host, ttl=600, renew_interval=120)
        try:
            self.assertIsNotNone(adopted)
            adopted.stop_renewer()
            self.assertFalse(adopted.lost, adopted.lost_reason)
            self.assertEqual(adopted._started_at, claim._started_at)
            self.assertTrue(adopted.renew())
            self.assertFalse(adopted.lost)
        finally:
            if adopted is not None:
                adopted.release()

    def test_real_renewer_thread_across_a_route_flip(self):
        # Wall clock, kept short: the renewer thread drives renew() at
        # 0.25 s against a 5 s TTL while the primary dies and comes back
        # under a 0.4 s sticky window. `lost` must stay False and the ref
        # must never be absent. Generous margins; if this ever flakes it
        # is named here, not hidden.
        fb = FallbackHub(self.primary, self.github, sticky_s=0.4)
        claim = Claim(
            fb, "gate", "tree-thread", work_key="staging/thread",
            rustc_id="r", platform_id="p", ttl=5, renew_interval=0.25,
        )
        observed = []
        stop = threading.Event()

        def watch():
            while not stop.is_set():
                observed.append(self.store.sha(claim.ref))
                time.sleep(0.05)

        claim.acquire()
        watcher = threading.Thread(target=watch, daemon=True)
        watcher.start()
        try:
            time.sleep(0.6)
            self.primary.kill()
            time.sleep(0.8)
            self.primary.restart()
            time.sleep(1.2)
            self.assertFalse(claim.lost, claim.lost_reason)
            self.assertTrue(claim.renewer_running())
        finally:
            stop.set()
            watcher.join(timeout=2)
            claim.release()
        self.assertNotIn(None, observed)
        self.assertGreaterEqual(len(set(observed)), 4)
        self.assertGreaterEqual(self.github.writes, 1)  # at least one direct renewal
        self.assertIn("update", self.primary_ops())  # and the route flipped back


if __name__ == "__main__":
    unittest.main()
