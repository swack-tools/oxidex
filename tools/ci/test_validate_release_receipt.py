"""Behavioral tests for release receipt shape and semantic validation."""

from __future__ import annotations

import copy
import hashlib
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
                self.assertEqual(validator.validate_schema(kind, fixture(kind)), [])
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
        payload["authenticated_reads"]["native_occurrence_floor_evidence"] = None
        self.assert_invalid(
            "parity", payload, "authenticated_reads.native_occurrence_floor_evidence"
        )
        payload = fixture("parity")
        payload["refusals"] = ["oracle fallback"]
        self.assert_invalid("parity", payload, "refusals")

        for dotted, value in (
            ("oracle.perl_version", "v5.40.0"),
            ("oracle.exiftool_version", "13.58"),
            ("oracle.docx_file_type", "ZIP"),
            ("oracle.probes", [None]),
            ("corpora", [None]),
            ("runs", [None]),
            ("conformance.artifacts", [None]),
            ("generated_catalog.ratchet_exit_code", 1),
            ("write_matrix.report_exit_code", 1),
            ("regressions.new_losses", 999),
        ):
            payload = fixture("parity")
            target = payload
            parts = dotted.split(".")
            for part in parts[:-1]:
                target = target[part]
            target[parts[-1]] = value
            with self.subTest(path=dotted, adversarial=True):
                self.assert_invalid("parity", payload, dotted)

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

        for dotted, value in (
            ("claims", [None]),
            ("pages", [None]),
            ("local_build.candidate_sha", "f" * 40),
            ("visual_review.samples", [None]),
            ("visual_review.findings", ["broken layout"]),
            ("pages_pipeline.workflow_sha", "not-a-sha"),
        ):
            payload = fixture("documentation")
            target = payload
            parts = dotted.split(".")
            for part in parts[:-1]:
                target = target[part]
            target[parts[-1]] = value
            with self.subTest(path=dotted, adversarial=True):
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

        for dotted, value in (
            ("packaging.prerelease", False),
            ("packaging.targets", [None]),
            ("version_inventory", [None]),
            ("gates", [None]),
            ("artifacts", [{"sha256": "3" * 64}]),
            ("macos_verification.dmg_payload_sha256", "6" * 64),
            ("macos_verification.reported_version", "oxidex 1.0.0"),
            ("macos_verification.release_run_id", 9999),
        ):
            payload = fixture("finalization")
            target = payload
            parts = dotted.split(".")
            for part in parts[:-1]:
                target = target[part]
            target[parts[-1]] = value
            with self.subTest(path=dotted, adversarial=True):
                self.assert_invalid("finalization", payload, dotted)

    def test_finalization_receipts_follow_candidate_or_regenerated_main_tree(self):
        payload = fixture("finalization")
        payload["receipts"]["parity"]["measured_sha"] = "f" * 40
        self.assert_invalid("finalization", payload, "receipts.parity.measured_sha")

        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            payload = fixture("finalization")
            payload["main_tree"] = "e" * 40
            for kind, identity, tree_field in (
                ("parity", "oxidex_sha", "oxidex_tree"),
                ("documentation", "candidate_sha", "candidate_tree"),
            ):
                upstream = fixture(kind)
                upstream[identity] = payload["main_sha"]
                upstream[tree_field] = payload["main_tree"]
                if kind == "parity":
                    upstream["regressions"]["head_sha"] = payload["main_sha"]
                else:
                    parity_path = root / "parity.json"
                    upstream["parity_receipt"]["path"] = str(parity_path)
                    upstream["parity_receipt"]["sha256"] = hashlib.sha256(
                        parity_path.read_bytes()
                    ).hexdigest()
                    upstream["parity_receipt"]["measured_sha"] = payload["main_sha"]
                    upstream["local_build"]["candidate_sha"] = payload["main_sha"]
                    upstream["pages_pipeline"]["workflow_sha"] = payload["main_sha"]
                path = root / f"{kind}.json"
                path.write_text(json.dumps(upstream), encoding="utf-8")
                payload["receipts"][kind]["path"] = str(path)
                payload["receipts"][kind]["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
                payload["receipts"][kind]["measured_sha"] = payload["main_sha"]
                payload["receipts"][kind]["measured_tree"] = payload["main_tree"]
            self.assertEqual(
                validator.validate_receipt(
                    "finalization", payload, expected_version=VERSION, expected_sha=SHA
                ),
                [],
            )

    def test_finalization_rejects_missing_tampered_or_stale_upstream_receipts(self):
        payload = fixture("finalization")
        payload["receipts"]["parity"]["path"] = "/missing/parity.json"
        self.assert_invalid("finalization", payload, "receipts.parity.path")

        payload = fixture("finalization")
        payload["receipts"]["parity"]["sha256"] = "f" * 64
        self.assert_invalid("finalization", payload, "receipts.parity.sha256")

        payload = fixture("finalization")
        payload["receipts"]["parity"]["measured_tree"] = "f" * 40
        self.assert_invalid("finalization", payload, "receipts.parity.measured_tree")

    def test_finalization_links_packaging_inventory_and_macos_artifacts(self):
        for dotted, value in (
            ("packaging.targets", ["made-up-target"]),
            ("packaging.expected_assets", ["made-up-asset"]),
            ("version_inventory[0].version", "1.0.0"),
            ("macos_verification.raw_binary_artifact", "wrong-name"),
            ("macos_verification.dmg_artifact", "wrong.dmg"),
        ):
            payload = fixture("finalization")
            if dotted.startswith("version_inventory"):
                payload["version_inventory"][0]["version"] = value
            else:
                target = payload
                parts = dotted.split(".")
                for part in parts[:-1]:
                    target = target[part]
                target[parts[-1]] = value
            with self.subTest(path=dotted):
                self.assert_invalid("finalization", payload, dotted.split("[")[0])

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

    def test_cli_requires_release_identity_outside_template_mode(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "receipt.json"
            path.write_text(json.dumps(fixture("parity")), encoding="utf-8")
            self.assertEqual(
                validator.main(["--kind", "parity", "--receipt", str(path)]), 2
            )


if __name__ == "__main__":
    unittest.main()
