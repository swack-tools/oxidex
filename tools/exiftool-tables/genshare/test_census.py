"""Black-box and selection contract tests for the maintained census entry point."""

import hashlib
import importlib.util
import json
import pathlib
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[3]
SCRATCH = ROOT / ".superpowers/sdd/task8-receipt-repair-design/test-tmp"
SCRATCH.mkdir(parents=True, exist_ok=True)
GENSHARE = ROOT / "tools/exiftool-tables/genshare"
SCRIPT = GENSHARE / "census.sh"
MODULE = GENSHARE / "attribute.py"
MANIFEST = GENSHARE / "testdata/bounded-corpus.txt"
EXPECTATIONS = GENSHARE / "testdata/bounded-corpus-expectations.json"
SPEC = importlib.util.spec_from_file_location("genshare_attribute_census", MODULE)
attribute = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(attribute)


class CensusSelectionTests(unittest.TestCase):
    def test_manifest_is_the_reviewed_three_file_bounded_corpus(self):
        rows = [row for row in MANIFEST.read_text(encoding="utf-8").splitlines() if row]
        self.assertEqual(rows, ["ICC_Profile.icc", "AAC.aac", "OOXML.docx"])
        expected = json.loads(EXPECTATIONS.read_text(encoding="utf-8"))
        self.assertEqual([row["relative_path"] for row in expected["fixtures"]], rows)
        self.assertEqual(
            [row["sha256"] for row in expected["fixtures"]],
            [
                "a8d0d753bd6129357cc2647435ce675e8637a679eb526fa180fba460874ce1d3",
                "5eac3eb5435a37bcaaf4f81eb0f37e487e50b65b67d486ac7d671c3f0a1d4671",
                "9d092c95902eb4b0cd7dc58252bd463441811b4478ebabf18895181c5b932662",
            ],
        )

    def test_bounded_selection_stages_a_directory_and_binds_source_bytes(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            base = pathlib.Path(td)
            corpus = base / "corpus"
            corpus.mkdir()
            for name, data in (("a.bin", b"a"), ("nested/b.bin", b"bb")):
                path = corpus / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            manifest = base / "manifest.txt"
            manifest.write_text("a.bin\nnested/b.bin\n", encoding="utf-8")
            run_root = base / "durable-run"
            selection = attribute.create_bounded_selection(corpus, manifest, run_root, 2)
            self.assertEqual(pathlib.Path(selection["selection_root"]), run_root / "selection")
            self.assertEqual(selection["ordered_paths"], ["a.bin", "nested/b.bin"])
            self.assertEqual((run_root / "selection/a.bin").read_bytes(), b"a")
            self.assertEqual(selection["ordered_manifest"][1]["size"], 2)
            self.assertEqual(
                selection["ordered_manifest"][1]["sha256"], hashlib.sha256(b"bb").hexdigest()
            )

    def test_manifest_refuses_escape_duplicate_symlink_and_existing_output(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            base = pathlib.Path(td)
            corpus = base / "corpus"
            corpus.mkdir()
            (corpus / "a").write_bytes(b"a")
            (corpus / "link").symlink_to(corpus / "a")
            cases = ["../a\n", "/a\n", "a\na\n", "link\n", "a\\b\n"]
            for index, text in enumerate(cases):
                manifest = base / f"m{index}.txt"
                manifest.write_text(text, encoding="utf-8")
                with self.subTest(text=text), self.assertRaises(attribute.ReceiptError):
                    attribute.create_bounded_selection(corpus, manifest, base / f"run{index}", 1)
            existing = base / "existing"
            existing.mkdir()
            marker = existing / "keep"
            marker.write_text("owned", encoding="utf-8")
            manifest = base / "ok.txt"
            manifest.write_text("a\n", encoding="utf-8")
            with self.assertRaisesRegex(attribute.ReceiptError, "already exists"):
                attribute.create_bounded_selection(corpus, manifest, existing, 1)
            self.assertEqual(marker.read_text(), "owned")


class CensusCliTests(unittest.TestCase):
    def _base_args(self, output, tokens):
        return [
            "bash", str(SCRIPT),
            "--repository", str(ROOT),
            "--target-dir", str(ROOT / "target-for-test"),
            "--output", str(output),
            "--corpus", "/does/not/exist",
            "--manifest", str(MANIFEST),
            "--min-files", "3",
            "--min-tags", "30",
            "--perl", "/does/not/exist",
            "--exiftool-dir", "/does/not/exist",
            "--tokens", tokens,
        ]

    def test_mixed_refused_token_starts_no_child_and_writes_failure(self):
        with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
            output = pathlib.Path(td) / "run"
            result = subprocess.run(
                self._base_args(output, "engine,legacy-l1,legacy-l2,producers,serial,conv"),
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            receipt = json.loads((output / "receipt.failed.json").read_text())
            self.assertEqual(receipt["schema"], "genshare-receipt/v3")
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["failed_stage"], "token-contract")
            self.assertEqual(receipt["failure"]["started_corpus_children"], 0)
            self.assertFalse((output / "selection").exists())
            validated = subprocess.run(
                [
                    "python3", str(MODULE), "validate-failure",
                    "--receipt", str(output / "receipt.failed.json"),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(validated.returncode, 0, validated.stderr)

    def test_explicit_manifest_and_floors_are_required(self):
        result = subprocess.run(
            ["bash", str(SCRIPT), "--repository", str(ROOT)],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("--manifest", result.stderr)
        self.assertIn("--min-files", result.stderr)
        self.assertIn("--min-tags", result.stderr)


if __name__ == "__main__":
    unittest.main()
