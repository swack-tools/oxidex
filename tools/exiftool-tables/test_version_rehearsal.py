#!/usr/bin/env python3
"""Offline contract tests for version_rehearsal.py; never use the network."""
from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("version_rehearsal", HERE / "version_rehearsal.py")
assert spec and spec.loader
vr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vr)

OID_A = "a" * 40
OID_B = "b" * 40
OID_C = "c" * 40
OID_D = "d" * 40
SHA_A = "1" * 64
SHA_B = "2" * 64
SHA_C = "3" * 64


def release(name, oid, sha):
    return {"name": name, "tag_object": oid, "peeled_commit": oid, "archive": {"url": f"https://github.com/exiftool/exiftool/archive/refs/tags/{name}.tar.gz", "sha256": sha}}


def catalog_entries():
    return {
        "catalog_source": {"kind": "official_exiftool_tag_catalog", "pages": [{"url": "https://api.github.com/repos/exiftool/exiftool/tags?per_page=100&page=1", "sha256": "0" * 64}]},
        "captured_at": "2026-09-13T00:00:00Z",
        "entries": [
            release("13.59", OID_C, SHA_C),
            {"name": "v13.58", "tag_object": OID_B},
            release("13.57", OID_A, SHA_A),
            {"name": "13.58", "tag_object": OID_B, "peeled_commit": OID_B, "archive": {"url": "https://github.com/exiftool/exiftool/archive/refs/tags/13.58.tar.gz"}},
            release("13.60", OID_D, SHA_B),
        ],
    }


class VersionRehearsalTests(unittest.TestCase):
    def normalized(self):
        return vr.normalize_catalog(catalog_entries())

    def plan(self, seed=7, sample_index=0, pair_count=1):
        return vr.make_plan(self.normalized(), seed, sample_index, pair_count, "e" * 40)

    @staticmethod
    def rehash_plan(plan):
        plan["plan_sha256"] = vr.sha256_json({key: value for key, value in plan.items() if key != "plan_sha256"})

    @staticmethod
    def rehash_catalog(catalog):
        catalog["catalog_sha256"] = vr.sha256_json({key: value for key, value in catalog.items() if key != "catalog_sha256"})

    def test_catalog_preserves_unclassified_and_excluded_entries(self):
        catalog = self.normalized()
        self.assertEqual(len(catalog["entries"]), 5)
        self.assertEqual(catalog["entries"][1]["classification"], {"state": "excluded", "reason": "tag_name_not_numeric_release"})
        self.assertEqual(catalog["entries"][3]["classification"], {"state": "eligible"})
        self.assertEqual([row["name"] for row in vr.eligible_releases(catalog)], ["13.57", "13.58", "13.59", "13.60"])

    def test_deterministic_replay_and_ordered_distinct_pairs(self):
        first = self.plan(seed=99, sample_index=4)
        second = self.plan(seed=99, sample_index=4)
        self.assertEqual(first, second)
        old, new = first["pairs"][0]["old"]["release"], first["pairs"][0]["new"]["release"]
        self.assertLess(vr.release_key(old), vr.release_key(new))
        self.assertNotEqual(old, new)
        self.assertEqual(first["execution"]["per_version_read_vs_native"], "unrun")
        self.assertEqual(first["execution"]["per_version_write_vs_native"], "unrun")
        self.assertEqual(first["execution"]["native_old_to_native_new_delta"], "unrun")
        pair = first["pairs"][0]
        self.assertEqual(pair["native_oracles"]["old"]["native_release_identity"], pair["old"])
        self.assertEqual(pair["native_oracles"]["new"]["native_release_identity"], pair["new"])
        self.assertEqual(pair["comparison_contract"]["cross_version_output_equality"], "not_required")
        self.assertTrue(pair["comparison_contract"]["newer_native_supersedes_older_native"])
        self.assertEqual(len(self.plan(seed=99, sample_index=4, pair_count=2)["pairs"]), 2)

    def test_same_version_and_ambiguous_identity_are_refused(self):
        catalog = self.normalized()
        duplicate = copy.deepcopy(catalog_entries())
        duplicate["entries"].extend([
            release("13.58", OID_D, SHA_B),
            release("13.59", OID_D, SHA_B),
            release("13.60", OID_D, SHA_B),
        ])
        duplicate_normalized = vr.normalize_catalog(duplicate)
        with self.assertRaisesRegex(vr.Refused, "at least two"):
            vr.eligible_releases(duplicate_normalized)
        bad_plan = self.plan()
        bad_plan["pairs"][0]["new"]["release"] = bad_plan["pairs"][0]["old"]["release"]
        # The immutable checksum catches mutation before the semantic check.
        with self.assertRaisesRegex(vr.Refused, "identity changed"):
            vr.verify_plan(bad_plan, self.normalized())

    def test_changed_catalog_identity_refuses_plan_reuse(self):
        catalog = self.normalized()
        plan = vr.make_plan(catalog, 3, 0, 1, "e" * 40)
        changed_raw = catalog_entries()
        changed_raw["entries"][0]["archive"]["sha256"] = "f" * 64
        changed = vr.normalize_catalog(changed_raw)
        with self.assertRaisesRegex(vr.Refused, "catalog identity differs"):
            vr.verify_plan(plan, changed)

    def test_swapped_or_same_native_oracle_is_refused_after_rehash(self):
        plan = self.plan()
        pair = plan["pairs"][0]
        pair["native_oracles"]["old"]["native_release_identity"] = copy.deepcopy(pair["new"])
        self.rehash_plan(plan)
        with self.assertRaisesRegex(vr.Refused, "deterministic catalog selection"):
            vr.verify_plan(plan, self.normalized())

    def test_unselected_eligible_release_is_explicitly_untested(self):
        catalog = self.normalized()
        plan = vr.make_plan(catalog, 12, 0, 1, "e" * 40)
        selected = {side["release"] for pair in plan["pairs"] for side in (pair["old"], pair["new"])}
        untested = {row["release"]: row["reason"] for row in plan["untested_eligible_releases"]}
        self.assertEqual(selected | set(untested), {"13.57", "13.58", "13.59", "13.60"})
        self.assertTrue(all(reason == "not_selected_by_seeded_pair_plan" for reason in untested.values()))

    def test_archive_presence_does_not_filter_pair_selection(self):
        catalog = self.normalized()
        plan = self.plan(seed=12)
        self.assertTrue(all(pair["source_resolution"]["state"] == "unresolved" for pair in plan["pairs"]))
        self.assertIn("13.58", {entry["name"] for entry in vr.eligible_releases(catalog)})

    def test_duplicate_run_outputs_are_refused_and_initial_status_is_all_unrun(self):
        plan = self.plan()
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            _, status = vr.create_run(plan, self.normalized(), run_dir)
            journal = json.loads(status.read_text())
            self.assertEqual(journal["phase"], "planned")
            self.assertTrue(all(v["read"] == v["write"] == v["state"] == "unrun" for v in journal["releases"].values()))
            with self.assertRaisesRegex(vr.Refused, "already exists"):
                vr.create_run(plan, self.normalized(), run_dir)

    def test_interrupted_recovery_preserves_unrun_and_failure_is_accounted(self):
        plan = self.plan()
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            vr.create_run(plan, self.normalized(), run_dir)
            vr.start_pair(run_dir, 0)
            selected = plan["pairs"][0]["old"]["release"]
            failed = vr.record_failure(run_dir, 0, selected, "write", "native validation failed")
            self.assertEqual(failed["phase"], "failed")
            self.assertEqual(failed["releases"][selected]["write"], "failed")
            self.assertEqual(failed["pairs"][0]["failure"]["detail"], "native validation failed")

    def test_interrupted_recovery_preserves_unrun(self):
        plan = self.plan()
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            vr.create_run(plan, self.normalized(), run_dir)
            vr.start_pair(run_dir, 0)
            recovered = vr.recover_interrupted(run_dir, self.normalized())
            self.assertEqual(recovered["phase"], "interrupted")
            self.assertTrue(all(v["read"] == v["write"] == "unrun" for v in recovered["releases"].values()))

    def test_rehashed_selector_or_scope_mutations_are_refused(self):
        for mutation in (
            lambda plan: plan.__setitem__("seed", plan["seed"] + 1),
            lambda plan: plan.__setitem__("pair_count", 2),
            lambda plan: plan.__setitem__("pairs", []),
            lambda plan: plan.__setitem__("untested_eligible_releases", []),
        ):
            with self.subTest(mutation=mutation):
                plan = self.plan()
                mutation(plan)
                self.rehash_plan(plan)
                with self.assertRaisesRegex(vr.Refused, "deterministic catalog selection"):
                    vr.verify_plan(plan, self.normalized())

    def test_rehashed_catalog_classification_mutation_is_refused(self):
        catalog = self.normalized()
        catalog["entries"][0]["classification"] = {"state": "excluded", "reason": "invented"}
        self.rehash_catalog(catalog)
        with self.assertRaisesRegex(vr.Refused, "catalog identity changed"):
            vr.verify_catalog(catalog)

    def test_journal_refuses_missing_extra_or_duplicate_pairs_releases_and_scope(self):
        plan = self.plan(pair_count=2)
        for mutate in (
            lambda journal: journal["pairs"].pop(),
            lambda journal: journal["pairs"].append(copy.deepcopy(journal["pairs"][0])),
            lambda journal: journal["pairs"].__setitem__(0, {**journal["pairs"][0], "old_release": "not-selected"}),
            lambda journal: journal["releases"].pop(next(iter(journal["releases"]))),
            lambda journal: journal["releases"].__setitem__("not-selected", {"read": "unrun", "write": "unrun", "state": "unrun"}),
            lambda journal: journal.__setitem__("untested_eligible_releases", [{"release": "not-selected", "reason": "invented"}]),
        ):
            with self.subTest(mutate=mutate), tempfile.TemporaryDirectory() as tmp:
                run_dir = Path(tmp) / "run"
                vr.create_run(plan, self.normalized(), run_dir)
                status_path = run_dir / "status.json"
                journal = json.loads(status_path.read_text())
                mutate(journal)
                status_path.write_text(json.dumps(journal))
                with self.assertRaisesRegex(vr.Refused, "journal (pairs|releases|untested scope)"):
                    vr.load_verified_run(run_dir)

    def test_failure_must_belong_to_active_pair(self):
        plan = self.plan(pair_count=2)
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            vr.create_run(plan, self.normalized(), run_dir)
            vr.start_pair(run_dir, 0)
            other = plan["pairs"][1]
            with self.assertRaisesRegex(vr.Refused, "active pair"):
                vr.record_failure(run_dir, other["pair_index"], other["old"]["release"], "read", "wrong active pair")

    def test_mutated_plan_and_journal_identity_are_refused(self):
        plan = self.plan()
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            vr.create_run(plan, self.normalized(), run_dir)
            plan_path = run_dir / "run-plan.json"
            altered = json.loads(plan_path.read_text())
            altered["repository_commit"] = "f" * 40
            plan_path.write_text(json.dumps(altered))
            with self.assertRaisesRegex(vr.Refused, "plan identity changed"):
                vr.load_verified_run(run_dir)

    def test_mutated_saved_catalog_is_refused_before_journal_use(self):
        plan = self.plan()
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            vr.create_run(plan, self.normalized(), run_dir)
            catalog_path = run_dir / "catalog.normalized.json"
            catalog = json.loads(catalog_path.read_text())
            catalog["entries"][0]["raw"]["archive"]["sha256"] = "f" * 64
            catalog_path.write_text(json.dumps(catalog))
            with self.assertRaisesRegex(vr.Refused, "catalog identity changed"):
                vr.load_verified_run(run_dir)

    def test_cli_plan_writes_no_success_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            catalog_path = base / "catalog.json"
            catalog_path.write_text(json.dumps(catalog_entries()))
            run_dir = base / "run"
            rc = vr.main(["plan", "--catalog", str(catalog_path), "--seed", "4", "--repository-commit", "e" * 40, "--run-dir", str(run_dir)])
            self.assertEqual(rc, 0)
            _, journal = vr.load_verified_run(run_dir)
            self.assertEqual(journal["phase"], "planned")
            self.assertTrue(all(item["read"] == item["write"] == "unrun" for item in journal["releases"].values()))

    def test_planning_journal_cannot_be_tampered_to_claim_success(self):
        plan = self.plan()
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = Path(tmp) / "run"
            vr.create_run(plan, self.normalized(), run_dir)
            status_path = run_dir / "status.json"
            journal = json.loads(status_path.read_text())
            release = next(iter(journal["releases"]))
            journal["releases"][release]["read"] = "passed"
            journal["releases"][release]["state"] = "passed"
            status_path.write_text(json.dumps(journal))
            with self.assertRaisesRegex(vr.Refused, "cannot claim read/write success"):
                vr.load_verified_run(run_dir)


if __name__ == "__main__":
    unittest.main()
