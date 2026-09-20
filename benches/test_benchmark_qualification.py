"""Fail-closed contracts for the frozen-candidate benchmark wrapper."""

from __future__ import annotations

import hashlib
import json
import re
from datetime import datetime, timezone
import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("benchmark_qualification.py")
WORKFLOW_PATH = MODULE_PATH.parents[1] / ".github" / "workflows" / "benchmark-qualification.yml"
SPEC = importlib.util.spec_from_file_location("benchmark_qualification", MODULE_PATH)
assert SPEC and SPEC.loader
qualification = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qualification)


SHA = "55090c9b1c86d90efa48719ee39a1c94177eb9a0"
BINARY_SHA = "a" * 64


def result_row(command: str, *, candidate: bool = False) -> dict:
    times = [0.01] * 30
    return {
        "command": command,
        "times": times,
        "exit_codes": [0] * 30,
        "summary": {"min": 0.01, "max": 0.01, "mean": 0.01, "median": 0.01, "stddev": 0.0},
        "candidate": candidate,
    }


def result_document(binary: str, corpus: Path, *, commit: str = SHA) -> dict:
    source_manifest = qualification.build_corpus_manifest(corpus, expected_count=2)
    evidence = corpus.parent / "evidence" / "run-20260920-a"
    evidence.parent.mkdir(exist_ok=True)
    evidence.mkdir(exist_ok=True)
    artifact_paths = {name: evidence / name for name in ("result.json", "raw.json", "source.json", "cache.json")}
    for artifact in artifact_paths.values():
        artifact.write_text("artifact")
    artifact_paths["source.json"].write_bytes(qualification._canonical_json(source_manifest))
    artifact_hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in artifact_paths.items()}
    measured_at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    commands = {
        "single_file": ["single-file-oracle", binary],
        "single_canon": ["single-canon-oracle", binary],
        "batch": ["batch-oracle", binary],
        "write": ["write-oracle", binary],
        "detection": ["detection-oracle", binary],
        "corpus": ["corpus-oracle " + " ".join(str(corpus / name) for name in ("a", "b")),
                   binary + " " + " ".join(str(corpus / name) for name in ("a", "b"))],
        "corpus_1thread": ["corpus-1thread-oracle " + " ".join(str(corpus / name) for name in ("a", "b")),
                           binary + " " + " ".join(str(corpus / name) for name in ("a", "b"))],
    }
    return {
        "identity": {"run_id": "run-20260920-a", "evidence_path": str(evidence),
                      "measured_at": measured_at,
                      "result_artifact_path": str(artifact_paths["result.json"]),
                      "raw_artifact_path": str(artifact_paths["raw.json"]),
                      "source_artifact_path": str(artifact_paths["source.json"]),
                      "cache_artifact_path": str(artifact_paths["cache.json"]),
                      "cache_policy": "warm-cache", "result_artifact_sha256": "f" * 64,
                      "source_artifact_sha256": artifact_hashes["source.json"],
                      "raw_artifact_sha256": artifact_hashes["raw.json"],
                      "cache_artifact_sha256": artifact_hashes["cache.json"]},
        "instrument": {
            "commit": commit,
            "dirty": False,
            "oxidex": binary,
            "oxidex_sha256": BINARY_SHA,
            "oxidex_version": "oxidex 2.0.0-beta.1",
            "staleness_note": "",
            "exiftool_version": "13.59",
        },
        **{
            name: {
                "results": [
                    result_row(commands[name][0]),
                    result_row(binary + " " + " ".join(str(corpus / name) for name in ("a", "b")) if name in ("corpus", "corpus_1thread") else binary, candidate=True),
                ]
            }
            for name in qualification.EXPECTED_SCENARIOS
        },
    }


class CorpusManifestTests(unittest.TestCase):
    def test_manifest_is_sorted_and_hashes_each_exact_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "b").write_bytes(b"bravo")
            (root / "a").write_bytes(b"alpha")

            manifest = qualification.build_corpus_manifest(root, expected_count=2)

            self.assertEqual([row["path"] for row in manifest["files"]], ["a", "b"])
            self.assertEqual(manifest["file_count"], 2)
            self.assertEqual(manifest["files"][0]["sha256"], hashlib.sha256(b"alpha").hexdigest())

    def test_manifest_rejects_symlinks_instead_of_following_them(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            target.write_bytes(b"target")
            (root / "alias").symlink_to(target)

            with self.assertRaisesRegex(qualification.Refused, "symlink"):
                qualification.build_corpus_manifest(root, expected_count=2)


class ResultValidationTests(unittest.TestCase):
    def _valid(self, root: Path) -> dict:
        (root / "a").write_bytes(b"a")
        (root / "b").write_bytes(b"b")
        return result_document("/fresh-target/release/oxidex", root)

    def test_duplicate_json_keys_are_rejected_at_artifact_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "result.json"
            path.write_text('{"identity": {}, "identity": {}}')
            with self.assertRaisesRegex(qualification.Refused, "duplicate"):
                qualification.load_json_rejecting_duplicates(path)

    def test_nonfinite_samples_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = self._valid(root)
            document["single_file"]["results"][1]["times"][0] = "not-a-number"
            manifest = qualification.build_corpus_manifest(root, expected_count=2)
            with self.assertRaisesRegex(qualification.Refused, "non-finite"):
                qualification.validate_result_document(document, candidate_sha=SHA,
                    binary_path=Path("/fresh-target/release/oxidex"), binary_sha256=BINARY_SHA,
                    cargo_version="2.0.0-beta.1", exiftool_version="13.59", corpus_manifest=manifest,
                    warmups=5, runs=30, expected_corpus_count=2)

    def test_canonical_artifact_hash_is_constructible_and_validates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = self._valid(root)
            path = root.parent / "evidence" / "run-20260920-a" / "result.json"
            document["identity"]["result_artifact_path"] = str(path.resolve())
            document["identity"]["result_artifact_sha256"] = qualification.canonical_result_sha256(document)
            path.write_bytes(qualification._canonical_json(document))
            # Canonical hash excludes only the binding field, so serialization is constructible.
            result = qualification.validate_result_artifact(
                path, candidate_sha=SHA, binary_path=Path("/fresh-target/release/oxidex"),
                binary_sha256=BINARY_SHA, cargo_version="2.0.0-beta.1", exiftool_version="13.59",
                corpus_manifest=qualification.build_corpus_manifest(root, expected_count=2),
                warmups=5, runs=30, expected_corpus_count=2)
            self.assertEqual(result["benchmark_status"], "observed-input-validated")

    def test_artifacts_must_stay_inside_run_and_be_fresh(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = self._valid(root)
            manifest = qualification.build_corpus_manifest(root, expected_count=2)
            document["identity"]["raw_artifact_path"] = str((root.parent / "outside.raw").resolve())
            (root.parent / "outside.raw").write_text("outside")
            with self.assertRaisesRegex(qualification.Refused, "outside"):
                qualification.validate_result_document(document, candidate_sha=SHA,
                    binary_path=Path("/fresh-target/release/oxidex"), binary_sha256=BINARY_SHA,
                    cargo_version="2.0.0-beta.1", exiftool_version="13.59", corpus_manifest=manifest,
                    warmups=5, runs=30, expected_corpus_count=2)

    def test_source_artifact_must_match_the_trusted_corpus_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = self._valid(root)
            manifest = qualification.build_corpus_manifest(root, expected_count=2)
            source = Path(document["identity"]["source_artifact_path"])
            source.write_bytes(b"arbitrary substituted source bytes")
            document["identity"]["source_artifact_sha256"] = qualification.sha256_file(source)

            with self.assertRaisesRegex(qualification.Refused, "trusted corpus manifest"):
                qualification.validate_result_document(document, candidate_sha=SHA,
                    binary_path=Path("/fresh-target/release/oxidex"), binary_sha256=BINARY_SHA,
                    cargo_version="2.0.0-beta.1", exiftool_version="13.59", corpus_manifest=manifest,
                    warmups=5, runs=30, expected_corpus_count=2)
            document = self._valid(root)
            document["identity"]["measured_at"] = "2020-01-01T00:00:00Z"
            with self.assertRaisesRegex(qualification.Refused, "freshness"):
                qualification.validate_result_document(document, candidate_sha=SHA,
                    binary_path=Path("/fresh-target/release/oxidex"), binary_sha256=BINARY_SHA,
                    cargo_version="2.0.0-beta.1", exiftool_version="13.59", corpus_manifest=manifest,
                    warmups=5, runs=30, expected_corpus_count=2)
    def test_exact_seven_scenarios_and_two_30_sample_rows_are_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            corpus = Path(directory)
            (corpus / "a").write_bytes(b"a")
            (corpus / "b").write_bytes(b"b")
            manifest = qualification.build_corpus_manifest(corpus, expected_count=2)
            document = result_document("/fresh-target/release/oxidex", corpus)

            summary = qualification.validate_result_document(
                document,
                candidate_sha=SHA,
                binary_path=Path("/fresh-target/release/oxidex"),
                binary_sha256=BINARY_SHA,
                cargo_version="2.0.0-beta.1",
                exiftool_version="13.59",
                corpus_manifest=manifest,
                warmups=5,
                runs=30,
                expected_corpus_count=2,
            )

            self.assertEqual(summary["scenario_count"], 7)
            self.assertEqual(summary["result_row_count"], 14)
            self.assertEqual(summary["benchmark_status"], "observed-input-validated")

    def test_historical_result_is_refused_even_when_its_rows_look_complete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            corpus = Path(directory)
            (corpus / "a").write_bytes(b"a")
            (corpus / "b").write_bytes(b"b")
            manifest = qualification.build_corpus_manifest(corpus, expected_count=2)
            document = result_document("/fresh-target/release/oxidex", corpus, commit="8f04e28812e6e3aaa55e45b59070fde1523d2a0c")

            with self.assertRaisesRegex(qualification.Refused, "candidate SHA"):
                qualification.validate_result_document(
                    document,
                    candidate_sha=SHA,
                    binary_path=Path("/fresh-target/release/oxidex"),
                    binary_sha256=BINARY_SHA,
                    cargo_version="2.0.0-beta.1",
                    exiftool_version="13.59",
                    corpus_manifest=manifest,
                    warmups=5,
                    runs=30,
                    expected_corpus_count=2,
                )

    def test_wrong_sample_count_or_stale_note_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            corpus = Path(directory)
            (corpus / "a").write_bytes(b"a")
            (corpus / "b").write_bytes(b"b")
            manifest = qualification.build_corpus_manifest(corpus, expected_count=2)
            document = result_document("/fresh-target/release/oxidex", corpus)
            document["instrument"]["staleness_note"] = "binary predates source"
            document["corpus"]["results"][0]["times"].pop()

            with self.assertRaisesRegex(qualification.Refused, "staleness|30 timed"):
                qualification.validate_result_document(
                    document,
                    candidate_sha=SHA,
                    binary_path=Path("/fresh-target/release/oxidex"),
                    binary_sha256=BINARY_SHA,
                    cargo_version="2.0.0-beta.1",
                    exiftool_version="13.59",
                    corpus_manifest=manifest,
                    warmups=5,
                    runs=30,
                    expected_corpus_count=2,
                )


class RunIdentityTests(unittest.TestCase):
    def test_run_directory_must_be_new_and_outside_the_repository(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = root / "evidence"
            existing = evidence / "run-1"
            existing.mkdir(parents=True)

            with self.assertRaisesRegex(qualification.Refused, "already exists"):
                qualification.require_new_run_directory(evidence, "run-1", root / "repo")

            with self.assertRaisesRegex(qualification.Refused, "outside"):
                qualification.require_new_run_directory(root / "repo", "run-2", root / "repo")


class CandidateIdentityTests(unittest.TestCase):
    def test_exact_clean_git_commit_is_accepted_as_the_candidate_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(["git", "init", "--quiet", str(repository)], check=True)
            subprocess.run(["git", "-C", str(repository), "config", "user.email", "test@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(repository), "config", "user.name", "Qualification Test"], check=True)
            (repository / "candidate.txt").write_text("candidate\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(repository), "add", "candidate.txt"], check=True)
            subprocess.run(["git", "-C", str(repository), "commit", "--quiet", "-m", "candidate"], check=True)
            candidate_sha = subprocess.run(
                ["git", "-C", str(repository), "rev-parse", "HEAD"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()

            self.assertEqual(
                qualification.git_identity(repository, candidate_sha),
                {"sha": candidate_sha, "dirty": False},
            )


class BinaryIdentityTests(unittest.TestCase):
    def test_binary_must_be_the_explicit_release_target_and_match_its_hash(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "fresh-target"
            binary = target / "release" / "oxidex"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"fresh beta binary")
            binary.chmod(0o755)
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()

            identity = qualification.validate_binary_identity(
                binary,
                digest,
                target,
                root / "repository",
            )
            self.assertEqual(identity["sha256"], digest)

            with self.assertRaisesRegex(qualification.Refused, "SHA-256|hash"):
                qualification.validate_binary_identity(binary, "b" * 64, target, root / "repository")

            other = root / "other" / "oxidex"
            other.parent.mkdir()
            other.write_bytes(binary.read_bytes())
            other.chmod(0o755)
            with self.assertRaisesRegex(qualification.Refused, "target-dir/release"):
                qualification.validate_binary_identity(other, digest, target, root / "repository")


class ReceiptTests(unittest.TestCase):
    def test_preflight_receipt_explicitly_stays_unrun(self) -> None:
        receipt = qualification.build_unrun_receipt(
            run_id="run-20260920-a",
            evidence_path=Path("/evidence/run-20260920-a"),
            candidate={"sha": SHA, "dirty": False},
            binary={"path": "/target/release/oxidex", "sha256": BINARY_SHA},
            cargo={"package_version": "2.0.0-beta.1", "cargo_version": "cargo 1.97.1"},
            oracle={"version": "13.59", "docx_probe": "DOCX", "archive_zip": True},
            corpus={"file_count": 194, "files": [], "manifest_sha256": "c" * 64},
            machine={"cache_policy": "warm-cache; no cold-cache claim"},
        )

        self.assertEqual(receipt["benchmark_status"], "not_run")
        self.assertIsNone(receipt["measured_at"])
        self.assertTrue(receipt["historical_results_untouched"])
        self.assertEqual(receipt["sampling"], {"warmups": 5, "timed_runs_per_row": 30})


class QualificationWorkflowTests(unittest.TestCase):
    def test_manual_preflight_is_candidate_bound_and_never_times_a_benchmark(self) -> None:
        self.assertTrue(
            WORKFLOW_PATH.is_file(),
            "candidate qualification must have a dedicated manual workflow",
        )
        workflow = WORKFLOW_PATH.read_text(encoding="utf-8")

        self.assertRegex(workflow, r"(?m)^  workflow_dispatch:$")
        self.assertRegex(
            workflow,
            r"(?m)^      candidate_sha:\n(?:^        [^\n]*\n)*^        required: true$",
        )
        self.assertIn("ref: ${{ inputs.candidate_sha }}", workflow)
        self.assertIn("CANDIDATE_SHA: ${{ inputs.candidate_sha }}", workflow)
        self.assertIn('test "$(git rev-parse HEAD)" = "$CANDIDATE_SHA"', workflow)
        self.assertIn('test -z "$(git status --porcelain=v1)"', workflow)
        self.assertIn("python3 -m unittest benches/test_benchmark_qualification.py -v", workflow)
        self.assertIn("cargo build --release --locked --bin oxidex", workflow)
        self.assertIn("uses: ./.github/actions/pinned-exiftool", workflow)
        self.assertIn("python3 benches/benchmark_qualification.py", workflow)
        self.assertIn('--candidate-sha "$CANDIDATE_SHA"', workflow)
        self.assertIn('--corpus "$EXIFTOOL_SOURCE/t/images"', workflow)
        self.assertIn('--docx "$EXIFTOOL_SOURCE/t/images/OOXML.docx"', workflow)
        self.assertIn("--warmups 5", workflow)
        self.assertIn("--runs 30", workflow)
        self.assertIn("name: benchmark-qualification-preflight", workflow)
        self.assertNotIn("exiftool_comparison.sh", workflow)
        self.assertNotRegex(workflow, re.compile(r"(?m)^          (?!#).*\bhyperfine\b", re.IGNORECASE))


class OracleProbeTests(unittest.TestCase):
    def test_oracle_probe_requires_explicit_perl_archive_zip_and_docx_capability(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl = root / "perl5.38.2"
            perl.write_text(
                "#!/bin/sh\n"
                "case \"$*\" in\n"
                "  *-MArchive::Zip*) exit 0 ;;\n"
                "  *-ver*) printf '13.59\\n' ;;\n"
                "  *-FileType*) printf 'DOCX\\n' ;;\n"
                "  *) exit 1 ;;\n"
                "esac\n"
            )
            perl.chmod(0o755)
            exiftool_dir = root / "exiftool"
            (exiftool_dir / "lib").mkdir(parents=True)
            (exiftool_dir / "exiftool").write_text("# pinned source marker\n")
            docx = root / "OOXML.docx"
            docx.write_bytes(b"docx")

            identity = qualification.oracle_identity(
                perl, exiftool_dir, docx, Path(__file__).parents[1]
            )

            self.assertEqual(identity["perl"], str(perl.resolve()))
            self.assertEqual(identity["version"], "13.59")
            self.assertEqual(identity["docx_probe"], "DOCX")
            self.assertTrue(identity["source_manifest_sha256"])


if __name__ == "__main__":
    unittest.main()
