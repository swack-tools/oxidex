"""Behavioral tests for the reviewed-promotion PR evidence gate."""

from __future__ import annotations

import json
import pathlib
import tempfile
import unittest

from tools.ci import release_pr_gate


SHA = "a" * 40


class ReleasePrGateTests(unittest.TestCase):
    def fixtures(self, root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
        state = root / "pr-state.json"
        threads = root / "review-threads.json"
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
        return state, threads

    def test_accepts_approved_exact_head_with_no_actionable_threads(self):
        with tempfile.TemporaryDirectory() as tmp:
            state, threads = self.fixtures(pathlib.Path(tmp))
            result = release_pr_gate.validate(state, threads, SHA)
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
                state, threads = self.fixtures(pathlib.Path(tmp))
                payload = json.loads(state.read_text())
                payload[field] = value
                state.write_text(json.dumps(payload), encoding="utf-8")
                with self.assertRaisesRegex(release_pr_gate.PrGateError, message):
                    release_pr_gate.validate(state, threads, SHA)

        for mutation, message in (("paginate", "pagination"), ("thread", "unresolved")):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                state, threads = self.fixtures(pathlib.Path(tmp))
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
                    release_pr_gate.validate(state, threads, SHA)


if __name__ == "__main__":
    unittest.main()
