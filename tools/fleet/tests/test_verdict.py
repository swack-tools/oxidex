#!/usr/bin/env python3
"""Unit tests for tools/fleet/verdict.py.

Plain `unittest`, standard library only -- no pytest in this environment.
CAS-cache tests run against a throwaway `git init --bare` repo, same
pattern as `test_fleetlib.py`; `setUp` asserts it lives under the system
temp directory before any test body runs, per ground rule 1 in the T1.2
brief ("never contact the production hub for CAS experiments"). The
admissibility tests build small real (non-bare) git repos via
`gitfixture.RepoBuilder`, also confined to the system temp directory.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shutil
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from fleetlib import Hub  # noqa: E402
from gitfixture import RepoBuilder, require_temp_path  # noqa: E402
import verdict  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
DOMAINS_TOML = REPO_ROOT / "fleet" / "domains.toml"


# --------------------------------------------------------------------- #
# compute_ids
# --------------------------------------------------------------------- #


class TestComputeIds(unittest.TestCase):
    def test_stripped_and_unstripped_differ(self):
        text = "rustc 1.97.1 (abc123 2026-01-01)\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.97.1\n"
        rustc_id, platform_id = verdict.compute_ids(text)
        self.assertNotEqual(rustc_id, platform_id)

    def test_rustc_id_is_stable_across_hosts_platform_id_is_not(self):
        """The whole point of the split: two hosts on the identical rustc
        release, differing only in the `host:` line, must compare equal
        under rustc_id and unequal under platform_id -- otherwise a Linux
        PASS could satisfy a macOS gate slot.
        """
        common = "rustc 1.97.1 (abc123 2026-01-01)\nbinary: rustc\nrelease: 1.97.1\nLLVM version: 18.1.0\n"
        mac_text = common + "host: aarch64-apple-darwin\n"
        linux_text = common + "host: x86_64-unknown-linux-gnu\n"

        mac_rustc_id, mac_platform_id = verdict.compute_ids(mac_text)
        linux_rustc_id, linux_platform_id = verdict.compute_ids(linux_text)

        self.assertEqual(mac_rustc_id, linux_rustc_id, "rustc_id must ignore the host line")
        self.assertNotEqual(mac_platform_id, linux_platform_id, "platform_id must carry the host line")

    def test_deterministic(self):
        text = "rustc 1.97.1\nhost: x86_64-unknown-linux-gnu\n"
        self.assertEqual(verdict.compute_ids(text), verdict.compute_ids(text))


# --------------------------------------------------------------------- #
# load_domains
# --------------------------------------------------------------------- #


class TestLoadDomains(unittest.TestCase):
    def test_parses_seeded_domains_toml(self):
        domains = verdict.load_domains(DOMAINS_TOML)
        self.assertEqual(
            domains,
            frozenset(
                {
                    "src/exiftool_tables/mod.rs",
                    "src/exiftool_tables/enabled.rs",
                    "src/exiftool_tables/binary_tables.rs",
                    "src/exiftool_tables/runtime.rs",
                    "tools/exiftool-tables/verify_subdirs.py",
                    "src/bin/jpeg-tag-matrix/baseline.json",
                }
            ),
        )

    def test_missing_array_raises(self):
        tmp = Path(tempfile.mkdtemp(prefix="domains-test-"))
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        bad = tmp / "domains.toml"
        bad.write_text("not_domains = []\n")
        with self.assertRaises(ValueError):
            verdict.load_domains(bad)

    def test_unquoted_entry_raises(self):
        tmp = Path(tempfile.mkdtemp(prefix="domains-test-"))
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        bad = tmp / "domains.toml"
        bad.write_text("domains = [\n  unquoted.rs,\n]\n")
        with self.assertRaises(ValueError):
            verdict.load_domains(bad)


# --------------------------------------------------------------------- #
# validate_payload / verdict_ref
# --------------------------------------------------------------------- #


def _good_payload(**overrides) -> dict:
    payload = {
        "tree_sha": "a" * 40,
        "base_tip": "b" * 40,
        "branch": "staging/example",
        "result": "PASS",
        "stage": "complete",
        "gate_version": "2",
        "rustc_id": "r" * 64,
        "platform_id": "p" * 64,
        "host": "server",
        "duration_s": 120,
        "write_set": ["src/foo.rs"],
    }
    payload.update(overrides)
    return payload


class TestValidatePayload(unittest.TestCase):
    def test_good_payload_has_no_problems(self):
        self.assertEqual(verdict.validate_payload(_good_payload()), [])

    def test_missing_field_reported(self):
        payload = _good_payload()
        del payload["base_tip"]
        problems = verdict.validate_payload(payload)
        self.assertTrue(any("base_tip" in p for p in problems))

    def test_bad_result_reported(self):
        problems = verdict.validate_payload(_good_payload(result="MAYBE"))
        self.assertTrue(any("result" in p for p in problems))

    def test_write_set_must_be_list(self):
        problems = verdict.validate_payload(_good_payload(write_set="src/foo.rs"))
        self.assertTrue(any("write_set" in p for p in problems))


class TestVerdictRef(unittest.TestCase):
    def test_builds_three_part_path(self):
        ref = verdict.verdict_ref("deadbeef", "2", "platformhash")
        self.assertEqual(ref, "refs/fleet/verdicts/deadbeef/2/platformhash")

    def test_rejects_slash_in_component(self):
        with self.assertRaises(ValueError):
            verdict.verdict_ref("dead/beef", "2", "platformhash")

    def test_rejects_empty_component(self):
        with self.assertRaises(ValueError):
            verdict.verdict_ref("", "2", "platformhash")


# --------------------------------------------------------------------- #
# Cache: lookup / store, against a throwaway bare-repo hub
# --------------------------------------------------------------------- #


class VerdictCacheTestCase(unittest.TestCase):
    """Base fixture: a throwaway bare repo standing in for the hub.

    Mirrors `test_fleetlib.py.FleetlibTestCase` -- same guard, same
    disposability contract. Never the production hub.
    """

    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="verdict-cache-test-")
        self.hub_path = str(Path(self._tmp_root) / "hub.git")
        self.workdir = str(Path(self._tmp_root) / "cache")

        import subprocess

        init = subprocess.run(["git", "init", "--quiet", "--bare", self.hub_path], capture_output=True)
        self.assertEqual(init.returncode, 0, msg=init.stderr.decode())

        require_temp_path(self.hub_path)
        self.assertNotIn("work2.oxidex.net", str(Path(self.hub_path).resolve()))

        self.hub = Hub(url=self.hub_path, workdir=self.workdir)

    def tearDown(self):
        shutil.rmtree(self._tmp_root, ignore_errors=True)

    def fresh_hub(self) -> Hub:
        """A second Hub instance with its own local cache, same remote --
        simulates a second host computing the same merge.
        """
        other_workdir = tempfile.mkdtemp(prefix="verdict-cache-test-cache2-")
        self.addCleanup(shutil.rmtree, other_workdir, ignore_errors=True)
        return Hub(url=self.hub_path, workdir=other_workdir)


class TestLookupStore(VerdictCacheTestCase):
    def test_lookup_before_any_store_is_none(self):
        self.assertIsNone(verdict.lookup(self.hub, "a" * 40, "2", "p" * 64))

    def test_store_then_lookup_round_trips(self):
        payload = _good_payload()
        outcome = verdict.store(self.hub, payload)
        self.assertEqual(outcome, "created")

        found = verdict.lookup(self.hub, payload["tree_sha"], payload["gate_version"], payload["platform_id"])
        self.assertIsNotNone(found)
        self.assertEqual(found["result"], "PASS")
        self.assertEqual(found["branch"], "staging/example")

    def test_second_host_reuses_first_hosts_verdict(self):
        """The throughput property the whole cache exists for: two hosts
        computing the identical merge derive the identical key, and the
        second one gets the first one's answer back instead of rebuilding.
        """
        payload = _good_payload()
        first_outcome = verdict.store(self.hub, payload)
        self.assertEqual(first_outcome, "created")

        second_hub = self.fresh_hub()
        found = verdict.lookup(second_hub, payload["tree_sha"], payload["gate_version"], payload["platform_id"])
        self.assertIsNotNone(found)
        self.assertEqual(found["result"], "PASS")

        # The second host storing the *same* result again should not
        # duplicate work or clobber anything -- it's a cache hit.
        second_outcome = verdict.store(second_hub, payload)
        self.assertEqual(second_outcome, "cache-hit")

    def test_identical_store_twice_is_a_cache_hit_not_a_duplicate(self):
        payload = _good_payload()
        self.assertEqual(verdict.store(self.hub, payload), "created")
        self.assertEqual(verdict.store(self.hub, payload), "cache-hit")

    def test_abort_is_never_served_by_lookup(self):
        payload = _good_payload(result="ABORT", stage="low-disk")
        outcome = verdict.store(self.hub, payload)
        self.assertEqual(outcome, "created")

        found = verdict.lookup(self.hub, payload["tree_sha"], payload["gate_version"], payload["platform_id"])
        self.assertIsNone(found, "an ABORT verdict must never be served as a cache hit")

    def test_abort_then_pass_is_a_retry_that_overwrites(self):
        """Replays the rb-s26 shape: one run ABORTs (rustc SIGKILL'd during
        the LTO link), a retry on the same tree actually PASSes. The retry
        must win -- ABORT is 'non-admissible but non-damning', not a
        permanent verdict for the key.
        """
        abort_payload = _good_payload(result="ABORT", stage="killed-process")
        self.assertEqual(verdict.store(self.hub, abort_payload), "created")
        self.assertIsNone(
            verdict.lookup(self.hub, abort_payload["tree_sha"], abort_payload["gate_version"], abort_payload["platform_id"])
        )

        pass_payload = _good_payload(result="PASS", stage="complete")
        outcome = verdict.store(self.hub, pass_payload)
        self.assertEqual(outcome, "retried-abort")

        found = verdict.lookup(self.hub, pass_payload["tree_sha"], pass_payload["gate_version"], pass_payload["platform_id"])
        self.assertIsNotNone(found)
        self.assertEqual(found["result"], "PASS")

    def test_conflicting_non_abort_results_refuse_not_overwrite(self):
        pass_payload = _good_payload(result="PASS")
        fail_payload = _good_payload(result="FAIL", stage="tests")

        self.assertEqual(verdict.store(self.hub, pass_payload), "created")
        outcome = verdict.store(self.hub, fail_payload)
        self.assertEqual(outcome, "conflict")

        # The original PASS must survive untouched -- a conflict is
        # reported, never silently resolved by picking a winner.
        found = verdict.lookup(self.hub, pass_payload["tree_sha"], pass_payload["gate_version"], pass_payload["platform_id"])
        self.assertEqual(found["result"], "PASS")

    def test_store_rejects_invalid_payload(self):
        payload = _good_payload()
        del payload["result"]
        with self.assertRaises(ValueError):
            verdict.store(self.hub, payload)

    def test_concurrent_store_from_n_threads_exactly_one_created(self):
        payload = _good_payload()
        hubs = [self.fresh_hub() for _ in range(6)]

        with ThreadPoolExecutor(max_workers=6) as pool:
            outcomes = list(pool.map(lambda h: verdict.store(h, payload), hubs))

        self.assertEqual(outcomes.count("created"), 1, f"exactly one racer should create; got {outcomes}")
        self.assertEqual(outcomes.count("cache-hit"), 5, f"the rest should see a cache hit; got {outcomes}")


# --------------------------------------------------------------------- #
# is_admissible
# --------------------------------------------------------------------- #


class AdmissibilityTestCase(unittest.TestCase):
    def setUp(self):
        self._tmp_root = tempfile.mkdtemp(prefix="verdict-admiss-test-")
        require_temp_path(self._tmp_root)
        self.addCleanup(shutil.rmtree, self._tmp_root, ignore_errors=True)
        self.repo = RepoBuilder(Path(self._tmp_root) / "repo")
        self.domains = frozenset({"src/exiftool_tables/mod.rs", "src/exiftool_tables/enabled.rs"})

    def base_verdict(self, **overrides) -> dict:
        payload = _good_payload(platform_id="target-platform")
        payload.update(overrides)
        return payload


class TestIsAdmissibleBasics(AdmissibilityTestCase):
    def test_true_when_base_tip_equals_current_tip_and_clean(self):
        tip = self.repo.commit({"README.md": "hello"}, "initial")
        verdict_payload = self.base_verdict(base_tip=tip, write_set=["src/foo.rs"])
        result = verdict.is_admissible(
            verdict_payload, tip, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertTrue(result.admissible, result)
        self.assertEqual(result.reason, "ok")

    def test_true_when_intervening_commits_are_disjoint_and_domain_free(self):
        base = self.repo.commit({"README.md": "hello"}, "initial")
        current = self.repo.commit({"src/unrelated.rs": "fn unrelated() {}"}, "unrelated change")
        verdict_payload = self.base_verdict(base_tip=base, write_set=["src/foo.rs"])
        result = verdict.is_admissible(
            verdict_payload, current, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertTrue(result.admissible, result)

    def test_false_result_not_pass(self):
        tip = self.repo.commit({"README.md": "hello"}, "initial")
        verdict_payload = self.base_verdict(base_tip=tip, result="FAIL")
        result = verdict.is_admissible(
            verdict_payload, tip, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "not-pass")

    def test_false_result_abort(self):
        tip = self.repo.commit({"README.md": "hello"}, "initial")
        verdict_payload = self.base_verdict(base_tip=tip, result="ABORT")
        result = verdict.is_admissible(
            verdict_payload, tip, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "not-pass")

    def test_false_platform_mismatch(self):
        tip = self.repo.commit({"README.md": "hello"}, "initial")
        verdict_payload = self.base_verdict(base_tip=tip, platform_id="some-other-platform")
        result = verdict.is_admissible(
            verdict_payload, tip, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "platform-mismatch")

    def test_false_not_ancestor(self):
        self.repo.commit({"README.md": "hello"}, "initial")
        other_root = RepoBuilder(Path(self._tmp_root) / "unrelated-repo")
        foreign_sha = other_root.commit({"x.txt": "x"}, "unrelated history")
        verdict_payload = self.base_verdict(base_tip=foreign_sha)
        result = verdict.is_admissible(
            verdict_payload,
            self.repo.sha(),
            repo=self.repo.path,
            target_platform_id="target-platform",
            domains=self.domains,
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "not-ancestor")

    def test_false_write_set_overlap_with_intervening_commit(self):
        base = self.repo.commit({"src/foo.rs": "fn foo() {}"}, "initial")
        current = self.repo.commit({"src/foo.rs": "fn foo() { /* changed */ }"}, "someone else touched foo.rs")
        verdict_payload = self.base_verdict(base_tip=base, write_set=["src/foo.rs"])
        result = verdict.is_admissible(
            verdict_payload, current, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "write-set-overlap")

    def test_false_branch_write_set_touches_domain(self):
        tip = self.repo.commit({"README.md": "hello"}, "initial")
        verdict_payload = self.base_verdict(base_tip=tip, write_set=["src/exiftool_tables/enabled.rs"])
        result = verdict.is_admissible(
            verdict_payload, tip, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "conflict-domain-branch")

    def test_false_intervening_commit_touches_domain(self):
        base = self.repo.commit({"README.md": "hello"}, "initial")
        current = self.repo.commit({"src/exiftool_tables/mod.rs": "// census bump"}, "sibling branch bumped census")
        verdict_payload = self.base_verdict(base_tip=base, write_set=["src/exiftool_tables/tables/other.rs"])
        result = verdict.is_admissible(
            verdict_payload, current, repo=self.repo.path, target_platform_id="target-platform", domains=self.domains
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "conflict-domain-intervening")

    def test_domains_argument_accepts_a_toml_path(self):
        tip = self.repo.commit({"README.md": "hello"}, "initial")
        verdict_payload = self.base_verdict(base_tip=tip, write_set=["src/exiftool_tables/enabled.rs"])
        result = verdict.is_admissible(
            verdict_payload, tip, repo=self.repo.path, target_platform_id="target-platform", domains=DOMAINS_TOML
        )
        self.assertFalse(result.admissible)
        self.assertEqual(result.reason, "conflict-domain-branch")


if __name__ == "__main__":
    unittest.main()
