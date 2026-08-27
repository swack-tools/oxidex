#!/usr/bin/env python3
"""Tests for tools/fleet/keel/cachedhub.py -- the three rules that make the
server's index PROVABLY a cache (SPEC §3.2, §3.3, §4.3 rule 1).

Everything runs against a throwaway `git init --bare` repo under the system
temp dir (asserted before any test body runs), with TWO independent
`fleetlib.Hub` clients on it: `self.store`, the one the `CachedHub` under
test wraps, and `self.direct`, a second client standing in for a runner
writing on the fallback route -- a writer whose changes the index cannot
see until a sweep, which is the whole reason the rules exist.

Every test here was checked to FAIL with its bug present (the commit
message names how): serving a claim from the index, dropping the sweep's
tick guard, touching the index before the store, and forgetting the stale
mark after an ambiguous write.

Run with:
    cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_cachedhub -v
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import claim as claim_mod  # noqa: E402
from _env import HermeticCase  # noqa: E402
from fleetlib import Hub, HubUnreachableError  # noqa: E402
from keel.cachedhub import (  # noqa: E402
    FRESH_PREFIXES,
    SOURCE_SWEEP,
    SOURCE_WRITE,
    CachedHub,
    RefEntry,
    RefIndex,
)

CLAIMS = "refs/fleet/claims/"


def _bare_repo(root: Path) -> str:
    path = root / "state.git"
    init = subprocess.run(["git", "init", "--quiet", "--bare", str(path)], capture_output=True)
    assert init.returncode == 0, init.stderr.decode()
    resolved = str(path.resolve())
    system_tmp = str(Path(tempfile.gettempdir()).resolve())
    assert resolved.startswith(system_tmp), f"fixture {resolved!r} is not under {system_tmp!r}"
    return str(path)


class CachedHubTestCase(HermeticCase):
    def setUp(self):
        super().setUp()
        self._root = Path(tempfile.mkdtemp(prefix="cachedhub-test-"))
        self.hub_url = _bare_repo(self._root)
        self.store = Hub(url=self.hub_url, workdir=self._root / "store-cache")
        self.direct = Hub(url=self.hub_url, workdir=self._root / "direct-cache")
        self.ns = f"refs/fleet/test/{uuid.uuid4().hex[:12]}/"
        self.claims_ns = f"{CLAIMS}gate/{uuid.uuid4().hex[:12]}-"
        self.cached = CachedHub(self.store)

    def tearDown(self):
        self.cached.stop_sweeper()
        shutil.rmtree(self._root, ignore_errors=True)

    def ref(self, name: str) -> str:
        return self.ns + name

    def claim_ref(self, name: str) -> str:
        return self.claims_ns + name

    def count_calls(self, method: str) -> list:
        """Replace `self.store.<method>` with a counting wrapper; returns the
        list the wrapper appends each call's args to."""
        calls: list = []
        real = getattr(self.store, method)

        def spy(*args, **kwargs):
            calls.append(args)
            return real(*args, **kwargs)

        setattr(self.store, method, spy)
        return calls

    def corrupt(self, ref: str, sha: str = "d" * 40, payload=None) -> None:
        """Plant a wrong entry in the index, far in the future on the tick
        clock so no observation in this test can repair it by the rule."""
        self.cached.index._entries[ref] = RefEntry(sha, payload, time.monotonic() + 1e6, time.time(), SOURCE_SWEEP)


# --------------------------------------------------------------------- #
# Drop-in surface
# --------------------------------------------------------------------- #


class TestDropInSurface(CachedHubTestCase):
    def test_hub_methods_and_attributes_are_present(self):
        for name in ("sha", "read", "read_with_sha", "list", "fetch_namespace", "create", "update", "delete",
                     "code_sha", "code_list", "push_ref", "push_code_ref", "push_tip_ref", "delete_code_ref"):
            self.assertTrue(callable(getattr(self.cached, name)), name)
        self.assertEqual(self.cached.url, self.store.url)
        self.assertEqual(self.cached.workdir, self.store.workdir)
        self.assertEqual(self.cached.code_url, self.store.code_url)
        self.assertEqual(self.cached.code_push_url, self.store.code_push_url)
        self.assertEqual(self.cached.tip_push_url, self.store.tip_push_url)

    def test_fresh_prefixes_track_claim_module(self):
        self.assertEqual(FRESH_PREFIXES, (claim_mod.CLAIMS_PREFIX.rstrip("/") + "/",))
        self.assertTrue(self.cached.is_fresh(claim_mod.claim_ref("gate", "x")))
        self.assertFalse(self.cached.is_fresh("refs/fleet/claimsX/gate/x"))
        self.assertFalse(self.cached.is_fresh(self.ref("x")))

    def test_hub_contract_values_are_unchanged(self):
        """False = lost race, None = absent, raise = transport -- through the cache."""
        self.assertIsNone(self.cached.sha(self.ref("absent")))
        self.assertEqual(self.cached.read_with_sha(self.ref("absent")), (None, None))
        self.assertTrue(self.cached.create(self.ref("a"), {"v": 1}))
        self.assertFalse(self.cached.create(self.ref("a"), {"v": 2}))
        self.assertFalse(self.cached.update(self.ref("a"), {"v": 3}, expect_sha="0" * 40))
        self.assertFalse(self.cached.delete(self.ref("a"), expect_sha="0" * 40))
        self.assertEqual(self.cached.read(self.ref("a"))["v"], 1)

        def boom(*a, **k):
            raise HubUnreachableError("injected")

        self.store.update = boom
        with self.assertRaises(HubUnreachableError):
            self.cached.update(self.ref("a"), {"v": 4}, expect_sha=self.cached.sha(self.ref("a")))

    def test_outside_namespace_is_always_live_and_never_indexed(self):
        outside = "refs/other/thing"
        calls = self.count_calls("sha")
        self.assertTrue(self.direct.create(outside, {"v": 1}))
        self.assertEqual(self.cached.sha(outside), self.direct.sha(outside))
        self.assertEqual(len(calls), 1)
        self.assertIsNone(self.cached.index.get(outside))


# --------------------------------------------------------------------- #
# Rule 1: write-through, store first, index only from the result
# --------------------------------------------------------------------- #


class TestWriteThrough(CachedHubTestCase):
    def test_create_is_indexed_from_the_store_readback(self):
        ref = self.ref("one")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        entry = self.cached.index.get(ref)
        self.assertIsNotNone(entry)
        self.assertEqual(entry.sha, self.direct.sha(ref))
        self.assertEqual(entry.source, SOURCE_WRITE)
        # The payload recorded is the COMMITTED one (with `_augment`'s
        # provenance), which only a readback can know.
        self.assertEqual(entry.payload, self.direct.read(ref))
        self.assertIn("written_by", entry.payload)

    def test_index_is_never_touched_before_the_store_cas_runs(self):
        """The store-first ordering, asserted at the instant of the push."""
        ref = self.ref("order")
        seen = {}
        real_create, real_update, real_delete = self.store.create, self.store.update, self.store.delete

        def create(r, payload, **kw):
            seen["create"] = self.cached.index.get(r)
            return real_create(r, payload, **kw)

        def update(r, payload, expect_sha, **kw):
            seen["update"] = self.cached.index.get(r)
            return real_update(r, payload, expect_sha, **kw)

        def delete(r, expect_sha, **kw):
            seen["delete"] = self.cached.index.get(r)
            return real_delete(r, expect_sha, **kw)

        self.store.create, self.store.update, self.store.delete = create, update, delete

        self.assertTrue(self.cached.create(ref, {"v": 1}))
        self.assertIsNone(seen["create"], "index held an entry before the create reached the store")
        after_create = self.cached.index.get(ref)

        self.assertTrue(self.cached.update(ref, {"v": 2}, expect_sha=after_create.sha))
        self.assertEqual(seen["update"], after_create, "index changed before the update reached the store")
        after_update = self.cached.index.get(ref)
        self.assertNotEqual(after_update.sha, after_create.sha)
        self.assertEqual(after_update.payload["v"], 2)

        self.assertTrue(self.cached.delete(ref, expect_sha=after_update.sha))
        self.assertEqual(seen["delete"], after_update, "index changed before the delete reached the store")
        self.assertTrue(self.cached.index.get(ref).is_tombstone)

    def test_lost_race_never_records_our_payload(self):
        ref = self.ref("race")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        ours = self.cached.sha(ref)
        # A direct writer moves the ref under us.
        self.assertTrue(self.direct.update(ref, {"v": "theirs"}, expect_sha=ours))
        theirs = self.direct.sha(ref)
        self.assertFalse(self.cached.update(ref, {"v": "mine"}, expect_sha=ours))
        entry = self.cached.index.get(ref)
        self.assertEqual(entry.sha, theirs, "a lost race must leave the index at the store's sha")
        self.assertEqual(entry.payload["v"], "theirs")
        self.assertEqual(self.cached.read(ref)["v"], "theirs")

    def test_ambiguous_write_marks_the_entry_stale_and_reads_go_live(self):
        ref = self.ref("ambiguous")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        before = self.cached.index.get(ref)
        real_update = self.store.update

        def update_then_die(r, payload, expect_sha, **kw):
            real_update(r, payload, expect_sha, **kw)  # the CAS executes...
            raise HubUnreachableError("timeout after send")  # ...and the reply is lost

        self.store.update = update_then_die
        with self.assertRaises(HubUnreachableError):
            self.cached.update(ref, {"v": 2}, expect_sha=before.sha)
        self.store.update = real_update

        entry = self.cached.index.get(ref)
        self.assertTrue(entry.stale, "an ambiguous write must not leave a servable entry")
        reads = self.count_calls("read_with_sha")
        sha, payload = self.cached.read_with_sha(ref)
        self.assertEqual(len(reads), 1, "a stale entry must be answered by the store")
        self.assertEqual(sha, self.direct.sha(ref))
        self.assertEqual(payload["v"], 2)
        self.assertFalse(self.cached.index.get(ref).stale, "a live read repairs the entry")
        # `sha()` on a stale entry goes live too.
        self.cached.index.mark_stale(ref)
        shas = self.count_calls("sha")
        self.assertEqual(self.cached.sha(ref), self.direct.sha(ref))
        self.assertEqual(len(shas), 1)

    def test_readback_failure_after_a_landed_write_returns_true_and_goes_stale(self):
        ref = self.ref("readback")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        real_read = self.store.read_with_sha

        def fail_readback(r):
            raise HubUnreachableError("readback lost")

        self.store.read_with_sha = fail_readback
        self.assertTrue(self.cached.update(ref, {"v": 2}, expect_sha=self.cached.sha(ref)))
        self.store.read_with_sha = real_read
        entry = self.cached.index.get(ref)
        self.assertTrue(entry.stale)
        self.assertEqual(entry.payload["v"], 1, "stale keeps the last known value for display only")
        self.assertEqual(self.cached.read(ref)["v"], 2, "but never serves it")

    def test_delete_tombstones_and_reads_need_no_store_call(self):
        ref = self.ref("gone")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        self.assertTrue(self.cached.delete(ref, expect_sha=self.cached.sha(ref)))
        shas, reads = self.count_calls("sha"), self.count_calls("read_with_sha")
        self.assertIsNone(self.cached.sha(ref))
        self.assertEqual(self.cached.read_with_sha(ref), (None, None))
        self.assertNotIn(ref, self.cached.list(self.ns))
        self.assertNotIn(ref, self.cached.fetch_namespace("refs/fleet"))
        self.assertEqual((len(shas), len(reads)), (0, 0))
        self.assertIsNone(self.direct.sha(ref))

    def test_index_served_read_returns_a_copy(self):
        ref = self.ref("copy")
        self.assertTrue(self.cached.create(ref, {"v": {"nested": 1}}))
        self.cached.read(ref)["v"]["nested"] = 99
        self.assertEqual(self.cached.read(ref)["v"]["nested"], 1)


# --------------------------------------------------------------------- #
# Rule 3: claims are read fresh (SPEC §4.3 r1)
# --------------------------------------------------------------------- #


class TestFreshClaims(CachedHubTestCase):
    def test_corrupt_index_entry_for_a_claim_is_ignored(self):
        """The acceptance test PLAN Stage 2 names: corrupt the index entry
        for a claim; `sha()` still returns the store's truth."""
        ref = self.claim_ref("corrupt")
        self.assertTrue(self.cached.create(ref, {"holder_host": "h", "started_at": "t"}))
        truth = self.direct.sha(ref)
        self.corrupt(ref, payload={"holder_host": "imposter", "started_at": "never"})
        self.assertEqual(self.cached.sha(ref), truth)
        sha, payload = self.cached.read_with_sha(ref)
        self.assertEqual(sha, truth)
        self.assertEqual(payload["holder_host"], "h")
        self.assertEqual(self.cached.read(ref)["holder_host"], "h")

    def test_negative_control_a_non_claim_ref_is_served_from_the_index(self):
        """Without this the test above proves nothing: it must be the
        CLAIM prefix, not the cache being off, that makes reads live."""
        ref = self.ref("noncl")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        self.corrupt(ref, payload={"v": "planted"})
        self.assertEqual(self.cached.sha(ref), "d" * 40)
        self.assertEqual(self.cached.read(ref)["v"], "planted")

    def test_every_claim_read_is_a_store_call(self):
        ref = self.claim_ref("counted")
        self.assertTrue(self.cached.create(ref, {"holder_host": "h", "started_at": "t"}))
        shas, reads = self.count_calls("sha"), self.count_calls("read_with_sha")
        for _ in range(3):
            self.cached.sha(ref)
            self.cached.read(ref)
            self.cached.read_with_sha(ref)
        self.assertEqual(len(shas), 3)
        self.assertEqual(len(reads), 6)
        self.assertEqual(self.cached.sha(self.claim_ref("absent")), None)
        self.assertEqual(len(shas), 4, "absence of a claim is asked of the store too")

    def test_direct_renewal_is_visible_without_a_sweep_for_claims_only(self):
        """A runner renewing on the fallback route is invisible to the
        index until a sweep -- acceptable for everything but a claim."""
        c_ref, p_ref = self.claim_ref("direct"), self.ref("plain")
        self.assertTrue(self.cached.create(c_ref, {"holder_host": "h", "started_at": "t"}))
        self.assertTrue(self.cached.create(p_ref, {"v": 1}))
        c_old, p_old = self.cached.sha(c_ref), self.cached.sha(p_ref)
        self.assertTrue(self.direct.update(c_ref, {"holder_host": "h", "started_at": "t", "renewed": 1}, expect_sha=c_old))
        self.assertTrue(self.direct.update(p_ref, {"v": 2}, expect_sha=p_old))
        self.assertEqual(self.cached.sha(c_ref), self.direct.sha(c_ref))
        self.assertEqual(self.cached.read(c_ref)["renewed"], 1)
        self.assertEqual(self.cached.sha(p_ref), p_old, "a plain ref is index-served (documented staleness)")
        self.cached.sweep()
        self.assertEqual(self.cached.sha(p_ref), self.direct.sha(p_ref))
        self.assertEqual(self.cached.read(p_ref)["v"], 2)

    def test_real_claim_renew_survives_a_stale_index_entry(self):
        """`claim.Claim.renew` against a CachedHub whose index entry for the
        claim is wrong. This is the killed-healthy-gate path (claim.py
        L644-690): a stale sha from the index would make `renew` adopt it,
        lose the CAS, and `_mark_lost`."""
        key = uuid.uuid4().hex[:8]
        c = claim_mod.Claim(self.cached, "gate", key, holder_host="tester", ttl=600, renew_interval=120)
        c.acquire()
        self.addCleanup(c.stop_renewer)
        ref = c.ref
        self.assertEqual(c._sha, self.direct.sha(ref), "acquire read its sha from the store")
        self.corrupt(ref, payload=self.direct.read(ref))
        self.assertTrue(c.renew(), c.lost_reason)
        self.assertFalse(c.lost, c.lost_reason)
        self.assertEqual(c._sha, self.direct.sha(ref))
        # And a renewal on the fallback route, then via the cache: one lease.
        self.assertTrue(self.direct.update(ref, self.direct.read(ref), expect_sha=self.direct.sha(ref)))
        self.corrupt(ref, payload=self.direct.read(ref))
        self.assertTrue(c.renew(), c.lost_reason)
        self.assertFalse(c.lost, c.lost_reason)


# --------------------------------------------------------------------- #
# Rule 2: monotonic sweep
# --------------------------------------------------------------------- #


class TestSweepMonotonic(CachedHubTestCase):
    def _stale_listing_around(self, during):
        """Patch the store's `fetch_namespace` so the listing the sweep
        applies was taken BEFORE `during()` ran -- the realistic
        interleaving: `ls-remote` starts, a write-through lands, the
        (now stale) listing arrives."""
        real = self.store.fetch_namespace

        def fetch(prefix):
            listing = real(prefix)
            self.store.fetch_namespace = real  # one stale sweep; later ones are honest
            during()
            return listing

        self.store.fetch_namespace = fetch

    def test_stale_listing_never_resurrects_a_write_through_delete(self):
        ref = self.ref("deleted")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        sha = self.cached.sha(ref)
        self._stale_listing_around(lambda: self.assertTrue(self.cached.delete(ref, expect_sha=sha)))
        report = self.cached.sweep()
        self.assertEqual(report.kept_newer, 1)
        self.assertIsNone(self.cached.sha(ref), "a listing older than our delete brought the ref back")
        self.assertEqual(self.cached.read_with_sha(ref), (None, None))
        self.assertNotIn(ref, self.cached.list(self.ns))
        self.assertIsNone(self.direct.sha(ref))
        # The tombstone outlives the stale sweep and is collected by a fresh one.
        self.assertTrue(self.cached.index.get(ref).is_tombstone)
        self.cached.sweep()
        self.assertIsNone(self.cached.index.get(ref))

    def test_stale_listing_never_regresses_a_write_through_update(self):
        ref = self.ref("updated")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        old = self.cached.sha(ref)
        self._stale_listing_around(lambda: self.assertTrue(self.cached.update(ref, {"v": 2}, expect_sha=old)))
        self.cached.sweep()
        entry = self.cached.index.get(ref)
        self.assertEqual(entry.sha, self.direct.sha(ref))
        self.assertNotEqual(entry.sha, old, "a listing older than our update regressed the index")
        self.assertEqual(entry.source, SOURCE_WRITE)
        self.assertEqual(self.cached.read(ref)["v"], 2)

    def test_stale_listing_never_resurrects_a_write_through_create_as_absent(self):
        ref = self.ref("created")
        self._stale_listing_around(lambda: self.assertTrue(self.cached.create(ref, {"v": 1})))
        self.cached.sweep()
        self.assertEqual(self.cached.sha(ref), self.direct.sha(ref), "a listing older than our create dropped the ref")

    def test_fresh_sweep_advances_to_what_the_store_reports(self):
        kept, changed, removed, added = (self.ref(n) for n in ("kept", "changed", "removed", "added"))
        for r in (kept, changed, removed):
            self.assertTrue(self.cached.create(r, {"name": r}))
        self.assertTrue(self.direct.update(changed, {"name": "changed2"}, expect_sha=self.direct.sha(changed)))
        self.assertTrue(self.direct.delete(removed, expect_sha=self.direct.sha(removed)))
        self.assertTrue(self.direct.create(added, {"name": "added"}))

        report = self.cached.sweep()
        self.assertEqual((report.added, report.advanced, report.removed), (1, 1, 1))
        self.assertGreaterEqual(report.refreshed, 1)
        self.assertEqual(self.cached.index.shas(self.ns), self.direct.fetch_namespace(self.ns))
        self.assertIsNone(self.cached.sha(removed))
        self.assertEqual(self.cached.sha(added), self.direct.sha(added))

        reads = self.count_calls("read_with_sha")
        self.assertEqual(self.cached.read(kept)["name"], kept)
        self.assertEqual(len(reads), 0, "an unchanged sha keeps its cached payload across a sweep")
        self.assertEqual(self.cached.read(changed)["name"], "changed2")
        self.assertEqual(self.cached.read(added)["name"], "added")
        self.assertEqual(len(reads), 2, "a changed/added sha fetches its payload once, coherently")
        self.assertEqual(self.cached.read(changed)["name"], "changed2")
        self.assertEqual(len(reads), 2)

    def test_sweep_failure_raises_and_leaves_the_index_alone(self):
        ref = self.ref("kept")
        self.assertTrue(self.cached.create(ref, {"v": 1}))
        before = self.cached.index.snapshot()

        def boom(prefix):
            raise HubUnreachableError("injected")

        self.store.fetch_namespace = boom
        with self.assertRaises(HubUnreachableError):
            self.cached.sweep()
        self.assertEqual(self.cached.index.snapshot(), before)
        self.assertEqual(self.cached.sweep_failures, 1)
        self.assertIn("injected", self.cached.last_sweep_error)

    def test_unbuilt_index_serves_nothing(self):
        """An empty index is not an empty namespace: until the first sweep
        succeeds every read is live."""
        ref = self.ref("early")
        self.assertTrue(self.direct.create(ref, {"v": 1}))
        unbuilt = CachedHub(self.store, build=False)
        self.assertEqual(unbuilt.sha(ref), self.direct.sha(ref))
        self.assertEqual(unbuilt.read(ref)["v"], 1)
        self.assertIn(ref, unbuilt.list(self.ns))
        unbuilt.sweep()
        self.assertIn(ref, unbuilt.index.shas())

    def test_sweeper_thread_sees_direct_writes_and_survives_failures(self):
        ref = self.ref("bg")
        self.cached.start_sweeper(interval=0.05)
        self.assertTrue(self.direct.create(ref, {"v": 1}))
        deadline = time.monotonic() + 10
        while self.cached.sha(ref) is None and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertEqual(self.cached.sha(ref), self.direct.sha(ref))

        real = self.store.fetch_namespace
        fail = threading.Event()
        fail.set()

        def flaky(prefix):
            if fail.is_set():
                raise HubUnreachableError("injected")
            return real(prefix)

        self.store.fetch_namespace = flaky
        deadline = time.monotonic() + 10
        while self.cached.sweep_failures == 0 and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertGreater(self.cached.sweep_failures, 0)
        self.assertTrue(self.cached.sweeper_running())
        fail.clear()
        sweeps = self.cached.sweeps
        deadline = time.monotonic() + 10
        while self.cached.sweeps == sweeps and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertGreater(self.cached.sweeps, sweeps, "the sweeper did not recover")
        self.cached.stop_sweeper()
        self.assertFalse(self.cached.sweeper_running())


class TestRefIndexRule(HermeticCase):
    """The ordering rule in isolation, with explicit ticks."""

    def setUp(self):
        super().setUp()
        self.t = 100.0
        self.index = RefIndex(clock=lambda: self.t)

    def test_observation_older_than_entry_is_ignored(self):
        self.assertTrue(self.index.observe("r", "a" * 40, {"v": 1}, started_tick=10))
        self.assertFalse(self.index.observe("r", "b" * 40, {"v": 2}, started_tick=5))
        self.assertEqual(self.index.get("r").sha, "a" * 40)
        self.assertFalse(self.index.observe("r", "b" * 40, {"v": 2}, started_tick=10), "equal ticks do not replace")
        self.assertTrue(self.index.observe("r", "b" * 40, {"v": 2}, started_tick=11))
        self.assertEqual(self.index.get("r").payload, {"v": 2})

    def test_delete_tombstone_blocks_an_older_listing(self):
        self.index.observe("r", "a" * 40, {"v": 1}, started_tick=10)
        self.t = 20.0
        self.index.record_delete("r")
        report = self.index.apply_sweep({"r": "a" * 40}, started_tick=15)
        self.assertEqual(report.kept_newer, 1)
        self.assertIsNone(self.index.get("r").sha)
        self.assertEqual(self.index.shas(), {})
        # A listing that started after the delete reflects it; the
        # tombstone is collected. A listing after that which carries the
        # ref again is a legitimate recreation.
        self.assertEqual(self.index.apply_sweep({}, started_tick=25).removed, 0)
        self.assertIsNone(self.index.get("r"))
        self.index.record_delete("r")
        self.index.apply_sweep({"r": "c" * 40}, started_tick=30)
        self.assertEqual(self.index.get("r").sha, "c" * 40)

    def test_sweep_keeps_payload_on_unchanged_sha_and_drops_it_on_change(self):
        self.index.observe("r", "a" * 40, {"v": 1}, started_tick=10)
        self.index.apply_sweep({"r": "a" * 40}, started_tick=11)
        self.assertEqual(self.index.get("r").payload, {"v": 1})
        self.assertEqual(self.index.get("r").source, SOURCE_SWEEP)
        self.index.apply_sweep({"r": "b" * 40}, started_tick=12)
        self.assertIsNone(self.index.get("r").payload)
        self.assertEqual(self.index.get("r").sha, "b" * 40)

    def test_stale_entry_is_repaired_only_by_a_later_observation(self):
        self.index.observe("r", "a" * 40, {"v": 1}, started_tick=10)
        self.t = 20.0
        self.index.mark_stale("r")
        self.assertTrue(self.index.get("r").stale)
        self.assertFalse(self.index.observe("r", "a" * 40, {"v": 1}, started_tick=15))
        self.assertTrue(self.index.get("r").stale)
        self.index.apply_sweep({"r": "a" * 40}, started_tick=15)
        self.assertTrue(self.index.get("r").stale, "a sweep older than the mark must not clear it")
        self.index.apply_sweep({"r": "a" * 40}, started_tick=21)
        self.assertFalse(self.index.get("r").stale)
        self.assertIsNone(self.index.get("r").payload, "a stale payload is not trusted even on an unchanged sha")

    def test_sweep_is_scoped_to_its_namespace(self):
        self.index.observe("refs/fleet/x", "a" * 40, None, started_tick=10)
        self.index.observe("refs/other/y", "b" * 40, None, started_tick=10)
        self.index.apply_sweep({}, started_tick=11, namespace="refs/fleet/")
        self.assertIsNone(self.index.get("refs/fleet/x"))
        self.assertIsNotNone(self.index.get("refs/other/y"))


# --------------------------------------------------------------------- #
# Index vs store under concurrent writers (real OS processes)
# --------------------------------------------------------------------- #


def _direct_writer(hub_url: str, ns: str, claim_ref: str, idx: int, rounds: int) -> dict:
    """A runner on the fallback route: its own refs, CAS contention on a
    shared ref, and renewals of a shared claim -- all invisible to the
    index until it sweeps (or, for the claim, never relied upon)."""
    workdir = tempfile.mkdtemp(prefix=f"cachedhub-race-{idx}-")
    wins = 0
    try:
        hub = Hub(url=hub_url, workdir=workdir)
        for k in range(rounds):
            assert hub.create(f"{ns}w{idx}/r{k}", {"w": idx, "k": k})
            if k % 2:
                sha = hub.sha(f"{ns}w{idx}/r{k - 1}")
                assert hub.delete(f"{ns}w{idx}/r{k - 1}", expect_sha=sha)
            for target in (f"{ns}shared", claim_ref):
                while True:
                    sha, payload = hub.read_with_sha(target)
                    if sha is None:
                        ok = hub.create(target, {"by": idx, "n": 0})
                    else:
                        ok = hub.update(target, {"by": idx, "n": payload["n"] + 1}, expect_sha=sha)
                    if ok:
                        wins += 1
                        break
        return {"idx": idx, "wins": wins}
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


class TestIndexAgreesWithStoreUnderConcurrentWriters(CachedHubTestCase):
    WORKERS = 6
    ROUNDS = 4

    def test_after_the_race_and_a_sweep_index_equals_store(self):
        shared, claim_ref = self.ref("shared"), self.claim_ref("race")
        claim_store_calls = self.count_calls("sha")
        claim_samples = []
        stop = threading.Event()

        def sample_claim():
            while not stop.is_set():
                claim_samples.append(self.cached.sha(claim_ref))
                time.sleep(0.01)

        sampler = threading.Thread(target=sample_claim, daemon=True)
        sampler.start()

        with ProcessPoolExecutor(max_workers=self.WORKERS) as pool:
            futures = [
                pool.submit(_direct_writer, self.hub_url, self.ns, claim_ref, i, self.ROUNDS)
                for i in range(self.WORKERS)
            ]
            # Meanwhile this process writes THROUGH the cache on the same refs.
            own_wins = 0
            for k in range(self.ROUNDS):
                self.assertTrue(self.cached.create(self.ref(f"server/r{k}"), {"k": k}))
                while True:
                    sha, payload = self.cached.read_with_sha(shared)
                    if sha is None:
                        ok = self.cached.create(shared, {"by": "server", "n": 0})
                    else:
                        ok = self.cached.update(shared, {"by": "server", "n": payload["n"] + 1}, expect_sha=sha)
                    if ok:
                        own_wins += 1
                        break
            results = [f.result() for f in as_completed(futures)]
        stop.set()
        sampler.join(timeout=10)

        self.assertEqual(len(results), self.WORKERS, "a writer never reported back")
        worker_wins = sum(r["wins"] for r in results)
        self.assertEqual(worker_wins, self.WORKERS * self.ROUNDS * 2)

        # r1 under contention: every claim sample was a store call -- the
        # index never answered one -- and the samples only ever moved
        # forward with the store (n is monotonic through the CAS chain).
        self.assertGreater(len(claim_samples), 0)
        claim_calls = [c for c in claim_store_calls if c and c[0] == claim_ref]
        self.assertEqual(len(claim_calls), len(claim_samples))

        # The shared ref ended where the CAS chain says it must: every win
        # incremented `n` exactly once.
        final = self.direct.read(shared)
        self.assertEqual(final["n"], self.WORKERS * self.ROUNDS + own_wins - 1)

        # Before the sweep the index may lag the direct writers; it must
        # never contradict the store for anything it served us.
        self.cached.sweep()
        truth = self.direct.fetch_namespace("refs/fleet")
        self.assertEqual(self.cached.index.shas("refs/fleet/"), truth)
        self.assertEqual(self.cached.fetch_namespace("refs/fleet"), truth)
        self.assertEqual(self.cached.list(self.ns), self.direct.list(self.ns))
        for ref in truth:
            self.assertEqual(self.cached.read_with_sha(ref), self.direct.read_with_sha(ref), ref)
        self.assertEqual(len(self.cached.index.snapshot(include_tombstones=True)), len(truth),
                         "a fresh sweep leaves no tombstones behind")


if __name__ == "__main__":
    unittest.main()
