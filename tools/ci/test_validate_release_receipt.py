"""Behavioral tests for release receipt shape and semantic validation."""

from __future__ import annotations

import copy
import json
import pathlib
import tempfile
import unittest

from tools.ci import validate_release_receipt as validator


REPO = pathlib.Path(__file__).resolve().parents[2]
FIXTURES = REPO / "tools/ci/testdata/release_receipts"
TEMPLATES = {
    "parity": REPO / ".claude/skills/exiftool-parity/templates/release-parity-receipt.json",
    "documentation": REPO / ".claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.json",
    "finalization": REPO / ".claude/skills/oxidex-release-finalization/templates/release-finalization-receipt.json",
}
SHA = "a" * 40
VERSION = "2.0.0-beta.1"


def fixture(kind: str) -> dict:
    return json.loads((FIXTURES / f"{kind}-verified.json").read_text(encoding="utf-8"))


class ReleaseReceiptValidationTests(unittest.TestCase):
    def assert_invalid(self, kind: str, payload: dict, path: str) -> None:
        errors = validator.validate_receipt(
            kind, payload, expected_version=VERSION, expected_sha=SHA
        )
        self.assertTrue(any(error.startswith(path) for error in errors), errors)

    def test_templates_validate_only_in_template_mode(self):
        for kind, path in TEMPLATES.items():
            payload = json.loads(path.read_text(encoding="utf-8"))
            with self.subTest(kind=kind):
                self.assertEqual(validator.validate_receipt(kind, payload, template=True), [])
                self.assertTrue(validator.validate_receipt(kind, payload))

    def test_verified_fixtures_pass_and_bind_identity(self):
        for kind in TEMPLATES:
            with self.subTest(kind=kind):
                self.assertEqual(
                    validator.validate_receipt(
                        kind, fixture(kind), expected_version=VERSION, expected_sha=SHA
                    ),
                    [],
                )

        bad = fixture("parity")
        bad["oxidex_sha"] = "f" * 40
        self.assert_invalid("parity", bad, "oxidex_sha")

    def test_parity_requires_capable_oracle_all_families_and_satisfied_floor(self):
        cases = (
            ("oracle.status", "blocked"),
            ("conformance.status", "unverified"),
            ("authenticated_reads.status", "unverified"),
            ("generated_catalog.status", "unverified"),
            ("write_matrix.status", "unverified"),
        )
        for dotted, value in cases:
            payload = fixture("parity")
            target = payload
            parts = dotted.split(".")
            for part in parts[:-1]:
                target = target[part]
            target[parts[-1]] = value
            with self.subTest(path=dotted):
                self.assert_invalid("parity", payload, dotted)

        payload = fixture("parity")
        payload["authenticated_reads"]["metric_c"] = 7999
        self.assert_invalid("parity", payload, "authenticated_reads.metric_c")
        payload = fixture("parity")
        payload["refusals"] = ["oracle fallback"]
        self.assert_invalid("parity", payload, "refusals")

    def test_documentation_requires_upstream_hash_browser_human_and_pages_proof(self):
        for dotted, value in (
            ("parity_receipt.sha256", None),
            ("local_build.status", "unverified"),
            ("visual_review.status", "unverified"),
            ("visual_review.human_review.status", "unverified"),
            ("pages_pipeline.status", "unverified"),
            ("pages_pipeline.build_type", "branch"),
        ):
            payload = fixture("documentation")
            target = payload
            parts = dotted.split(".")
            for part in parts[:-1]:
                target = target[part]
            target[parts[-1]] = value
            with self.subTest(path=dotted):
                self.assert_invalid("documentation", payload, dotted)

    def test_finalization_requires_receipts_review_authorization_tag_workflows_and_macos(self):
        for dotted, value in (
            ("receipts.parity.sha256", None),
            ("promotion.unresolved_actionable_threads", 1),
            ("authorization.authorized_by", None),
            ("tag_verification.status", "unverified"),
            ("workflows", []),
            ("artifacts", []),
            ("macos_verification.payload_match", False),
            ("macos_verification.gatekeeper_status", "rejected"),
            ("macos_verification.stapler_status", "unverified"),
        ):
            payload = fixture("finalization")
            target = payload
            parts = dotted.split(".")
            for part in parts[:-1]:
                target = target[part]
            target[parts[-1]] = value
            with self.subTest(path=dotted):
                self.assert_invalid("finalization", payload, dotted)

        payload = fixture("finalization")
        payload["workflows"][0]["head_sha"] = "f" * 40
        self.assert_invalid("finalization", payload, "workflows[0].head_sha")

    def test_cli_returns_two_and_prints_field_paths_for_invalid_receipt(self):
        payload = fixture("parity")
        payload["authenticated_reads"]["native_occurrence_floor"] = 0
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "receipt.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            self.assertEqual(
                validator.main(
                    [
                        "--kind", "parity", "--receipt", str(path),
                        "--version", VERSION, "--candidate-sha", SHA,
                    ]
                ),
                2,
            )


if __name__ == "__main__":
    unittest.main()
