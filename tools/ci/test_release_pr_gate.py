"""Behavioral tests for the reviewed-promotion PR evidence gate."""

from __future__ import annotations

import json
import hashlib
import pathlib
import tempfile
import unittest

from tools.ci import release_pr_gate


SHA = "a" * 40
BASE = "b" * 40
TREE = "c" * 40


class ReleasePrGateTests(unittest.TestCase):
    def local_review(self, root: pathlib.Path, base: str = BASE, tree: str = TREE) -> pathlib.Path:
        result_path = root / "review_result.md"
        files = {
            "review_receipt": {"role": "review", "status": "completed", "exit_code": 0,
                               "base": base, "head": SHA, "tree": tree, "dirty": "",
                               "command": ["codex", "--output-last-message", str(result_path)],
                               "after": {"head": SHA, "tree": tree, "dirty": ""}},
            "review_result": "No actionable findings.\n",
            "findings": {"status": "reviewed", "base": base, "head": SHA, "tree": tree,
                         "unresolved_actionable_findings": 0, "findings": []},
            "authorization": {"authorization": "User: local Codex fallback authorized"},
        }
        acceptance = {"schema_version": 1, "status": "accepted", "base": base,
                      "head": SHA, "tree": tree, "unresolved_actionable_findings": 0,
                      "result_disposition": "no_unresolved_actionable_findings",
                      "reviewer": "maintainer", "reviewed_at": "2026-09-30T00:00:00Z"}
        for label, value in files.items():
            path = root / f"{label}.{'md' if label == 'review_result' else 'json'}"
            path.write_text(value if isinstance(value, str) else json.dumps(value), encoding="utf-8")
            acceptance[f"{label}_path"] = str(path)
            acceptance[f"{label}_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        path = root / "local-review.json"
        path.write_text(json.dumps(acceptance), encoding="utf-8")
        return path

    def fixtures(self, root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path, pathlib.Path]:
        state = root / "pr-state.json"
        threads = root / "review-threads.json"
        checks = root / "required-checks.json"
        state.write_text(
            json.dumps(
                {"baseRefName": "main", "headRefOid": SHA, "reviewDecision": "APPROVED"}
            ),
            encoding="utf-8",
        )
        threads.write_text(
            json.dumps(
                {"data": {"repository": {"pullRequest": {"reviewThreads": {
                    "pageInfo": {"hasNextPage": False},
                    "nodes": [
                        {"isResolved": True, "isOutdated": False},
                        {"isResolved": False, "isOutdated": True},
                    ],
                }}}}}
            ),
            encoding="utf-8",
        )
        checks.write_text(
            json.dumps([{"name": "Build & Test", "state": "SUCCESS", "link": "https://example.test/check"}]),
            encoding="utf-8",
        )
        return state, threads, checks

    def test_accepts_approved_exact_head_with_no_actionable_threads(self):
        with tempfile.TemporaryDirectory() as tmp:
            state, threads, checks = self.fixtures(pathlib.Path(tmp))
            result = release_pr_gate.validate(state, threads, checks, SHA)
            self.assertEqual(result["status"], "verified")
            self.assertEqual(result["unresolved_actionable_threads"], 0)

    def test_rejects_wrong_base_head_review_pagination_and_thread(self):
        cases = (
            ("baseRefName", "develop", "baseRefName"),
            ("headRefOid", "b" * 40, "headRefOid"),
            ("reviewDecision", "REVIEW_REQUIRED", "reviewDecision"),
        )
        for field, value, message in cases:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as tmp:
                state, threads, checks = self.fixtures(pathlib.Path(tmp))
                payload = json.loads(state.read_text())
                payload[field] = value
                state.write_text(json.dumps(payload), encoding="utf-8")
                with self.assertRaisesRegex(release_pr_gate.PrGateError, message):
                    release_pr_gate.validate(state, threads, checks, SHA)

        for mutation, message in (("paginate", "pagination"), ("thread", "unresolved")):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                state, threads, checks = self.fixtures(pathlib.Path(tmp))
                payload = json.loads(threads.read_text())
                review_threads = payload["data"]["repository"]["pullRequest"]["reviewThreads"]
                if mutation == "paginate":
                    review_threads["pageInfo"]["hasNextPage"] = True
                else:
                    review_threads["nodes"].append(
                        {"isResolved": False, "isOutdated": False}
                    )
                threads.write_text(json.dumps(payload), encoding="utf-8")
                with self.assertRaisesRegex(release_pr_gate.PrGateError, message):
                    release_pr_gate.validate(state, threads, checks, SHA)

    def test_rejects_missing_or_failed_required_checks(self):
        for payload in ([], [{"name": "Build", "state": "FAILURE", "link": "x"}]):
            with self.subTest(payload=payload), tempfile.TemporaryDirectory() as tmp:
                state, threads, checks = self.fixtures(pathlib.Path(tmp))
                checks.write_text(json.dumps(payload), encoding="utf-8")
                with self.assertRaisesRegex(release_pr_gate.PrGateError, "required-checks"):
                    release_pr_gate.validate(state, threads, checks, SHA)

    def test_rejects_non_object_pr_state_cleanly(self):
        with tempfile.TemporaryDirectory() as tmp:
            state, threads, checks = self.fixtures(pathlib.Path(tmp))
            state.write_text("[]", encoding="utf-8")
            with self.assertRaisesRegex(release_pr_gate.PrGateError, "pr-state.*object"):
                release_pr_gate.validate(state, threads, checks, SHA)

    def test_local_review_fallback_keeps_actual_github_decision(self):
        for decision, role in ((None, "review"), ("", "acceptance"), ("REVIEW_REQUIRED", "review")):
            with self.subTest(decision=decision, role=role), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                state, threads, checks = self.fixtures(root)
                payload = json.loads(state.read_text())
                payload.update(baseRefOid=BASE, reviewDecision=decision)
                state.write_text(json.dumps(payload), encoding="utf-8")
                local = self.local_review(root)
                receipt_path = root / "review_receipt.json"
                receipt = json.loads(receipt_path.read_text())
                receipt["role"] = role
                receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
                acceptance = json.loads(local.read_text())
                acceptance["review_receipt_sha256"] = hashlib.sha256(receipt_path.read_bytes()).hexdigest()
                local.write_text(json.dumps(acceptance), encoding="utf-8")
                result = release_pr_gate.validate(state, threads, checks, SHA, local, BASE, TREE)
                self.assertEqual(result["review_basis"], "local_review_fallback")
                self.assertEqual(result["review_decision"], decision)

    def test_local_review_rejects_changes_requested_stale_or_tampered_evidence(self):
        for mutation, message in (
            ("changes_requested", "reviewDecision"),
            ("missing_decision", "reviewDecision"),
            ("stale_base", "baseRefOid"),
            ("stale_tree", "local-review.tree"),
            ("missing_acceptance", "local-review"),
            ("tamper_acceptance", "local-review.status"),
            ("tamper_result", "review_result.*SHA-256"),
            ("failed_review", "review_receipt.status"),
            ("unresolved_finding", "findings.findings"),
        ):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                state, threads, checks = self.fixtures(root)
                payload = json.loads(state.read_text())
                payload.update(baseRefOid=BASE, reviewDecision="")
                local = self.local_review(root)
                if mutation == "changes_requested":
                    payload["reviewDecision"] = "CHANGES_REQUESTED"
                elif mutation == "missing_decision":
                    del payload["reviewDecision"]
                elif mutation == "stale_base":
                    payload["baseRefOid"] = "d" * 40
                elif mutation == "stale_tree":
                    record = json.loads(local.read_text())
                    record["tree"] = "d" * 40
                    local.write_text(json.dumps(record), encoding="utf-8")
                elif mutation == "missing_acceptance":
                    local.unlink()
                elif mutation == "tamper_acceptance":
                    record = json.loads(local.read_text())
                    record["status"] = "unverified"
                    local.write_text(json.dumps(record), encoding="utf-8")
                elif mutation == "tamper_result":
                    (root / "review_result.md").write_text("altered", encoding="utf-8")
                elif mutation in ("failed_review", "unresolved_finding"):
                    label = "review_receipt" if mutation == "failed_review" else "findings"
                    path = root / f"{label}.json"
                    data = json.loads(path.read_text())
                    if mutation == "failed_review":
                        data["status"] = "interrupted"
                    else:
                        data["findings"] = [{"id": "P1", "disposition": "open", "reason": "unfixed"}]
                    path.write_text(json.dumps(data), encoding="utf-8")
                    record = json.loads(local.read_text())
                    record[f"{label}_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
                    local.write_text(json.dumps(record), encoding="utf-8")
                state.write_text(json.dumps(payload), encoding="utf-8")
                with self.assertRaisesRegex(release_pr_gate.PrGateError, message):
                    release_pr_gate.validate(state, threads, checks, SHA, local, BASE, TREE)

    def test_local_review_keeps_ci_and_thread_gates(self):
        for mutation, message in (("failed_ci", "required-checks"), ("open_thread", "unresolved")):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                state, threads, checks = self.fixtures(root)
                state.write_text(json.dumps({"baseRefName": "main", "baseRefOid": BASE,
                                             "headRefOid": SHA, "reviewDecision": ""}), encoding="utf-8")
                local = self.local_review(root)
                if mutation == "failed_ci":
                    checks.write_text(
                        json.dumps([{"name": "Build", "state": "FAILURE", "link": "x"}]),
                        encoding="utf-8",
                    )
                else:
                    payload = json.loads(threads.read_text())
                    payload["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"].append(
                        {"isResolved": False, "isOutdated": False}
                    )
                    threads.write_text(json.dumps(payload), encoding="utf-8")
                with self.assertRaisesRegex(release_pr_gate.PrGateError, message):
                    release_pr_gate.validate(state, threads, checks, SHA, local, BASE, TREE)


if __name__ == "__main__":
    unittest.main()
