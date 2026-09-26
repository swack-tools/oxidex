"""Behavioral tests for release receipt shape and semantic validation.

Tree-scan binding of the positive fixtures
------------------------------------------
A real finalization receipt must record the version-literal and version-field
scans (``git grep`` over every tracked file) of the tree it releases, and the
validator recomputes both scans live and rejects any mismatch. Committing those
live values into the positive fixtures made every unrelated PR that added a
version literal anywhere (a test, a doc) stale the fixtures and fail CI on every
other open PR. So the committed fixtures carry obviously-placeholder scan values
(``"1" * 64``, ``"2" * 64``, counts of 1), and ``fixture("finalization")``
binds an in-memory copy to the checkout's live scans at test time -- writing a
bound copy of the reconciliation record to a temp file and re-pointing
``version_reconciliation.path``/``sha256`` at it. The validator itself is
unchanged: ``RawFixtureTreeBindingTests`` proves the placeholders are rejected,
and ``TreeScanIndependenceTests`` proves a receipt bound to one tree is rejected
once an unrelated version literal is added, while a freshly bound one passes.
There is nothing to refresh after an unrelated change.
"""

from __future__ import annotations

import copy
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

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
RECONCILIATION_FIXTURE = FIXTURES / "version-reconciliation-verified.json"
# The committed placeholders. They are schema-valid on purpose, so validating
# the raw fixture isolates the semantic tree-scan binding (see
# RawFixtureTreeBindingTests); they must never be refreshed to live values.
PLACEHOLDER_SCAN = {
    "scan_sha256": "1" * 64,
    "tracked_files": 1,
    "matching_lines": 1,
    "fields_scan_sha256": "2" * 64,
    "fields_tracked_files": 1,
    "fields_matching_lines": 1,
    "reconciled_files": 1,
    "reconciled_lines": 1,
    "reconciled_field_files": 1,
    "reconciled_field_lines": 1,
}
LITERAL_SCAN_FIELDS = frozenset(
    {"scan_sha256", "tracked_files", "matching_lines", "reconciled_files", "reconciled_lines"}
)
_BOUND_DIR: tempfile.TemporaryDirectory | None = None


def setUpModule() -> None:
    global _BOUND_DIR
    _BOUND_DIR = tempfile.TemporaryDirectory(prefix="receipt-bound-")


def tearDownModule() -> None:
    global _BOUND_DIR
    if _BOUND_DIR is not None:
        _BOUND_DIR.cleanup()
        _BOUND_DIR = None


def raw_fixture(kind: str) -> dict:
    return json.loads((FIXTURES / f"{kind}-verified.json").read_text(encoding="utf-8"))


def live_scan_binding() -> dict:
    """The ten reconciliation fields, computed by the validator's own scanners."""

    literal = validator._version_literal_scan()
    fields = validator._version_fields_scan()
    return {
        "scan_sha256": literal["sha256"],
        "tracked_files": literal["tracked_files"],
        "matching_lines": literal["matching_lines"],
        "fields_scan_sha256": fields["sha256"],
        "fields_tracked_files": fields["tracked_files"],
        "fields_matching_lines": fields["matching_lines"],
        "reconciled_files": literal["tracked_files"],
        "reconciled_lines": literal["matching_lines"],
        "reconciled_field_files": fields["tracked_files"],
        "reconciled_field_lines": fields["matching_lines"],
    }


def bind_to_scan(payload: dict, binding: dict, workdir: pathlib.Path) -> dict:
    """Bind a finalization receipt (and its reconciliation record) to ``binding``.

    Writes the bound reconciliation record to ``workdir`` and re-points the
    receipt's path/sha256 at it, so the nested hash chain stays consistent.
    """

    record = json.loads(RECONCILIATION_FIXTURE.read_text(encoding="utf-8"))
    record.update(binding)
    path = workdir / "version-reconciliation-verified.json"
    path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    payload["version_reconciliation"].update(binding)
    payload["version_reconciliation"]["path"] = str(path)
    payload["version_reconciliation"]["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    return payload


def fixture(kind: str) -> dict:
    payload = raw_fixture(kind)
    if kind == "finalization":
        assert _BOUND_DIR is not None, "module fixture not set up"
        bind_to_scan(payload, live_scan_binding(), pathlib.Path(_BOUND_DIR.name))
    return payload


def _clear_scan_caches() -> None:
    validator._version_literal_scan.cache_clear()
    validator._version_fields_scan.cache_clear()


def _git(*args: str, env: dict | None = None, stdin: bytes | None = None) -> str:
    return subprocess.run(
        ["git", *args], cwd=REPO, env=env, input=stdin, check=True, capture_output=True
    ).stdout.decode().strip()


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

    def test_documentation_not_applicable_benchmark_requires_reason(self):
        payload = fixture("documentation")
        payload["benchmarks"] = [
            {
                "disposition": "not_applicable",
                "reason": "No performance claim is published for this candidate.",
            }
        ]
        self.assertEqual(
            validator.validate_receipt(
                "documentation",
                payload,
                expected_version=VERSION,
                expected_sha=SHA,
            ),
            [],
        )

        payload["benchmarks"][0]["reason"] = ""
        self.assert_invalid("documentation", payload, "benchmarks[0].reason")

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

        payload = fixture("finalization")
        payload["packaging"]["targets"].pop()
        self.assert_invalid("finalization", payload, "packaging.targets")

        payload = fixture("finalization")
        payload["version_inventory"].pop()
        self.assert_invalid("finalization", payload, "version_inventory")

        payload = fixture("finalization")
        payload["version_inventory"][0]["path"] = "/does/not/exist/Cargo.toml"
        self.assert_invalid("finalization", payload, "version_inventory[0].path")

        payload = fixture("finalization")
        payload["version_inventory"][1]["kind"] = "independent_package"
        payload["version_inventory"][1]["disposition"] = "independent"
        self.assert_invalid("finalization", payload, "version_inventory[1].kind")

        payload = fixture("finalization")
        payload["version_inventory"][-1]["kind"] = "oxidex_package"
        payload["version_inventory"][-1]["disposition"] = "current"
        self.assert_invalid("finalization", payload, "version_inventory[8].kind")

        payload = fixture("finalization")
        payload["version_inventory"][0]["line"] = 68
        payload["version_inventory"][0]["evidence_path"] = "README.md"
        self.assert_invalid("finalization", payload, "version_inventory[0].line")
        self.assert_invalid("finalization", payload, "version_inventory[0].evidence_path")

        payload = fixture("finalization")
        payload["version_reconciliation"]["scan_sha256"] = "0" * 64
        self.assert_invalid("finalization", payload, "version_reconciliation.scan_sha256")

        payload = fixture("finalization")
        payload["version_reconciliation"]["fields_scan_sha256"] = "0" * 64
        self.assert_invalid("finalization", payload, "version_reconciliation.fields_scan_sha256")

        payload = fixture("finalization")
        payload["version_reconciliation"]["sha256"] = "f" * 64
        self.assert_invalid("finalization", payload, "version_reconciliation.sha256")

        payload = fixture("finalization")
        payload["version_reconciliation"]["reviewer"] = ""
        self.assert_invalid("finalization", payload, "version_reconciliation.reviewer")

        payload = fixture("finalization")
        mac = payload["macos_verification"]
        mac["raw_binary_artifact"], mac["dmg_artifact"] = (
            mac["dmg_artifact"], mac["raw_binary_artifact"]
        )
        mac["raw_binary_sha256"], mac["dmg_sha256"] = (
            mac["dmg_sha256"], mac["raw_binary_sha256"]
        )
        mac["dmg_payload_sha256"] = mac["raw_binary_sha256"]
        self.assert_invalid(
            "finalization", payload, "macos_verification.raw_binary_artifact"
        )

    def test_release_asset_contract_matches_workflow(self):
        workflow = (REPO / ".github/workflows/release.yml").read_text(encoding="utf-8")
        for target in validator.REQUIRED_TARGETS:
            with self.subTest(target=target):
                self.assertIn(target, workflow)
        for asset in (
            "oxidex-x86_64-unknown-linux-musl",
            "oxidex-aarch64-unknown-linux-musl",
            "oxidex-x86_64-pc-windows-gnu.exe",
            "oxidex-universal-apple-darwin",
            "oxidex-v${{ steps.version.outputs.version }}.dmg",
        ):
            with self.subTest(asset=asset):
                self.assertIn(asset, workflow)
        prepare = workflow.split("- name: Prepare release assets", 1)[1].split(
            "- name: Extract version and create release notes", 1
        )[0]
        copies = {
            line.strip()
            for line in prepare.splitlines()
            if line.strip().startswith("cp ")
        }
        self.assertEqual(
            copies,
            {
                "cp artifacts/oxidex-x86_64-unknown-linux-musl-${{ github.run_id }}-${{ github.run_attempt }}/oxidex-x86_64-unknown-linux-musl release-assets/",
                "cp artifacts/oxidex-aarch64-unknown-linux-musl-${{ github.run_id }}-${{ github.run_attempt }}/oxidex-aarch64-unknown-linux-musl release-assets/",
                "cp artifacts/oxidex-x86_64-pc-windows-gnu-${{ github.run_id }}-${{ github.run_attempt }}/oxidex-x86_64-pc-windows-gnu.exe release-assets/",
                "cp artifacts/oxidex-universal-apple-darwin-${{ github.run_id }}-${{ github.run_attempt }}/oxidex-universal-apple-darwin release-assets/",
            },
        )
        self.assertIn("find artifacts/oxidex-dmg-${{ github.run_id }}-${{ github.run_attempt }}", prepare)

    def test_finalization_requires_universal_binary_and_complete_asset_inventory(self):
        payload = fixture("finalization")
        payload["packaging"]["targets"].remove("x86_64-apple-darwin")
        self.assert_invalid("finalization", payload, "packaging.targets")
        for missing in ("SHA256SUMS", f"oxidex-v{VERSION}.sbom.cdx.json"):
            payload = fixture("finalization")
            payload["packaging"]["expected_assets"].remove(missing)
            payload["artifacts"] = [a for a in payload["artifacts"] if a["name"] != missing]
            self.assert_invalid("finalization", payload, "packaging.expected_assets")
        payload = fixture("finalization")
        payload["macos_verification"]["raw_binary_artifact"] = "oxidex-aarch64-apple-darwin"
        self.assert_invalid("finalization", payload, "macos_verification.raw_binary_artifact")

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


def _error_paths(errors: list[str]) -> set[str]:
    return {error.split(": ", 1)[0] for error in errors}


class RawFixtureTreeBindingTests(unittest.TestCase):
    """The production path binds to the live tree scan; placeholders never pass."""

    def test_committed_fixtures_carry_placeholders_and_a_consistent_hash_chain(self):
        record = json.loads(RECONCILIATION_FIXTURE.read_text(encoding="utf-8"))
        final = raw_fixture("finalization")["version_reconciliation"]
        for field, value in PLACEHOLDER_SCAN.items():
            with self.subTest(field=field):
                self.assertEqual(record[field], value)
                self.assertEqual(final[field], value)
        self.assertEqual(final["path"], RECONCILIATION_FIXTURE.relative_to(REPO).as_posix())
        self.assertEqual(
            final["sha256"], hashlib.sha256(RECONCILIATION_FIXTURE.read_bytes()).hexdigest()
        )

    def test_raw_fixture_is_rejected_exactly_for_its_tree_scan_binding(self):
        payload = raw_fixture("finalization")
        self.assertEqual(validator.validate_schema("finalization", payload), [])
        errors = validator.validate_receipt(
            "finalization", payload, expected_version=VERSION, expected_sha=SHA
        )
        self.assertEqual(
            _error_paths(errors),
            {f"version_reconciliation.{field}" for field in PLACEHOLDER_SCAN},
            errors,
        )

    def test_cli_rejects_placeholder_binding_and_accepts_live_binding(self):
        argv = ["--kind", "finalization", "--version", VERSION, "--candidate-sha", SHA]
        raw = FIXTURES / "finalization-verified.json"
        self.assertEqual(validator.main([*argv, "--receipt", str(raw)]), 2)
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "receipt.json"
            path.write_text(json.dumps(fixture("finalization")), encoding="utf-8")
            self.assertEqual(validator.main([*argv, "--receipt", str(path)]), 0)

    def test_production_scans_match_the_documented_gate_commands(self):
        # Recompute both scans the way references/gates.md tells an operator
        # to, independently of the validator's helpers, so binding the
        # fixtures to the validator's own scan is not self-referential.
        gates = (
            REPO / ".claude/skills/oxidex-release-finalization/references/gates.md"
        ).read_text(encoding="utf-8")
        documented = (
            (r"(^|[^[:alnum:]_])[vV]?[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?",
             validator._version_literal_scan),
            (r"\[package\]|version[[:space:]]*=|VERSION|__version__",
             validator._version_fields_scan),
        )
        # Both full-tree greps run concurrently; each takes seconds.
        greps = [
            subprocess.Popen(
                ["git", "grep", "-n", "-I", "-E", pattern, "--", ".",
                 ":(exclude)tools/ci/testdata/release_receipts/**"],
                cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            for pattern, _ in documented
        ]
        results = [(grep.communicate(), grep.returncode) for grep in greps]
        for (pattern, scanner), ((stdout, stderr), returncode) in zip(documented, results):
            with self.subTest(pattern=pattern):
                self.assertIn(pattern, gates)
                self.assertIn(returncode, (0, 1), stderr)
                lines = stdout.splitlines()
                self.assertGreater(len(lines), 0)
                self.assertEqual(
                    scanner(),
                    {
                        "sha256": hashlib.sha256(stdout).hexdigest(),
                        "matching_lines": len(lines),
                        "tracked_files": len({line.split(b":", 1)[0] for line in lines}),
                    },
                )


class TreeScanIndependenceTests(unittest.TestCase):
    """An unrelated version literal no longer stales the positive fixtures.

    The "other PR" is simulated without touching the checkout: a copy of the
    git index gains one extra tracked file whose blob lives in a throwaway
    object directory, marked assume-unchanged so ``git grep`` reads it from the
    object store. The validator's real scanners run unmodified against it; the
    scan delta assertions prove the simulated file was actually seen.
    """

    ADDED_PATH = "docs/unrelated-note-from-another-pr.md"
    ADDED_TEXT = b"This note mentions release 9.8.7 in passing.\n"

    def setUp(self):
        # Scans are cached per process; whatever this test leaves behind must
        # be recomputed by anything that runs after it.
        self.addCleanup(_clear_scan_caches)
        tmp = tempfile.TemporaryDirectory(prefix="receipt-tree-")
        self.addCleanup(tmp.cleanup)
        self.tmp = pathlib.Path(tmp.name)

    def _tree_with_added_file(self) -> dict:
        tracked = subprocess.run(
            ["git", "ls-files", "--error-unmatch", "--", self.ADDED_PATH],
            cwd=REPO, capture_output=True,
        )
        self.assertNotEqual(tracked.returncode, 0, f"{self.ADDED_PATH} is already tracked")
        index = (REPO / _git("rev-parse", "--git-path", "index")).resolve()
        objects = (REPO / _git("rev-parse", "--git-path", "objects")).resolve()
        (self.tmp / "objects").mkdir()
        (self.tmp / "index").write_bytes(index.read_bytes())
        env = {
            "GIT_INDEX_FILE": str(self.tmp / "index"),
            "GIT_OBJECT_DIRECTORY": str(self.tmp / "objects"),
            "GIT_ALTERNATE_OBJECT_DIRECTORIES": str(objects),
        }
        child = {**os.environ, **env}
        oid = _git("hash-object", "-w", "--stdin", env=child, stdin=self.ADDED_TEXT)
        _git("update-index", "--add", "--cacheinfo", f"100644,{oid},{self.ADDED_PATH}", env=child)
        _git("update-index", "--assume-unchanged", "--", self.ADDED_PATH, env=child)
        return env

    def validate(self, payload: dict) -> list[str]:
        return validator.validate_receipt(
            "finalization", payload, expected_version=VERSION, expected_sha=SHA
        )

    def test_unrelated_version_literal_keeps_fixture_valid_and_stale_receipt_fails(self):
        status_before = _git("status", "--porcelain")
        before = live_scan_binding()
        (self.tmp / "before").mkdir()
        stale = bind_to_scan(raw_fixture("finalization"), before, self.tmp / "before")
        self.assertEqual(self.validate(copy.deepcopy(stale)), [])

        with mock.patch.dict(os.environ, self._tree_with_added_file()):
            _clear_scan_caches()
            after = live_scan_binding()
            # The simulated PR really reached the production scanner: one more
            # file and line in the literal scan, the field scan untouched.
            self.assertEqual(after["tracked_files"], before["tracked_files"] + 1)
            self.assertEqual(after["matching_lines"], before["matching_lines"] + 1)
            self.assertNotEqual(after["scan_sha256"], before["scan_sha256"])
            for field in PLACEHOLDER_SCAN.keys() - LITERAL_SCAN_FIELDS:
                self.assertEqual(after[field], before[field], field)

            # A receipt recorded against the earlier tree is stale and rejected.
            errors = self.validate(copy.deepcopy(stale))
            self.assertEqual(
                _error_paths(errors),
                {f"version_reconciliation.{field}" for field in LITERAL_SCAN_FIELDS},
                errors,
            )
            # The positive fixture binds at test time, so it needs no refresh.
            self.assertEqual(self.validate(fixture("finalization")), [])
            self.assertEqual(
                validator.validate_schema("finalization", fixture("finalization")), []
            )

        self.assertEqual(_git("status", "--porcelain"), status_before)


if __name__ == "__main__":
    unittest.main()
