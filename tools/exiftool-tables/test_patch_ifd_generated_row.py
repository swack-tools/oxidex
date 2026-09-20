"""Transactional checks for the narrow generated IFD row producer."""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

import codegen
import others
import table_modules


ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools" / "exiftool-tables" / "patch_ifd_generated_row.py"


def source_document() -> dict:
    return {
        "exiftool_version": "13.59",
        "modules": {
            "Olympus": {
                "tables": {
                    "CameraSettings": {
                        "meta": {},
                        "tags": {
                            "2052": {
                                "Name": "StackedImage",
                                "Count": 2,
                                "Writable": "int32u",
                                "PrintConv": {
                                    "kind": "enum_partial",
                                    "map": {
                                        "0 0": "No",
                                        "1 *": "Live Composite (* images)",
                                        "9 *": "Focus-stacked (* images)",
                                    },
                                    "directives": {
                                        "OTHER": {
                                            "__perl": "CODE",
                                            "__deparse": others.OLYMPUS_STACKED_IMAGE_OTHER,
                                        }
                                    },
                                },
                            }
                        },
                    }
                }
            }
        },
    }


def emitted_row(document: dict) -> tuple[str, dict]:
    table = document["modules"]["Olympus"]["tables"]["CameraSettings"]
    literal, refusal = codegen.gen_ifd_tag_literal(
        table["tags"]["2052"],
        0x0804,
        codegen.new_ifd_stats(),
        set(),
        codegen.IfdGenContext.from_doc(document),
        table["meta"],
    )
    if literal is None or refusal is not None:
        raise AssertionError(f"synthetic source row was refused: {refusal!r}")
    _source, rows = codegen.gen_ifd_table(
        "Olympus",
        "CameraSettings",
        table,
        codegen.new_ifd_stats(),
        set(),
        codegen.IfdGenContext.from_doc(document),
        include_identity_ledger=True,
    )
    identity = {
        "module": "Olympus",
        "table": "CameraSettings",
        "full_name": "Image::ExifTool::Olympus::CameraSettings",
        **rows[0],
    }
    return literal, identity


def omitted_row(literal: str) -> str:
    literal = literal.replace(
        "omitted: Omitted::NONE",
        "omitted: Omitted { value_conv: false, raw_conv: false, condition: false, "
        "hook: false, subdirectory: false, print_conv: true }",
    )
    return re.sub(
        r"print_conv: PrintConv::StrEnum\(&\[.*\]\), subdir:",
        "print_conv: PrintConv::None, subdir:",
        literal,
    )


def counts(rows: list[dict]) -> dict:
    return {
        "rows": len(rows),
        "emitted": sum(row["artifact_state"] == "emitted" for row in rows),
        "refused": sum(row["artifact_state"] == "refused" for row in rows),
        "reader_eligible": sum(row["reader_state"] == "eligible" for row in rows),
        "reader_omitted": sum(row["reader_state"] == "omitted" for row in rows),
    }


class NarrowIfdRowProducer(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="oxidex-patch-ifd-row-")
        self.root = Path(self.temporary.name)
        self.document = source_document()
        self.source = self.root / "tables.json"
        self.source.write_text(json.dumps(self.document, sort_keys=True))
        self.literal, self.identity = emitted_row(self.document)

    def tearDown(self):
        self.temporary.cleanup()

    def artifact(self, literal: str) -> Path:
        directory = self.root / "ifd"
        directory.mkdir(exist_ok=True)
        (directory / "mod.rs").write_text("mod olympus;\n")
        row = directory / "olympus.rs"
        sibling = self.literal.replace("id: 0x0804", "id: 0x0803", 1).replace(
            'name: "StackedImage"', 'name: "StackedImageFixtureSibling"', 1
        )
        row.write_text(
            "pub static TABLE: IfdTable = IfdTable {\n"
            "    tags: &[\n"
            f"        {sibling},\n"
            f"        {literal},\n"
            "    ],\n"
            "};\n"
        )
        return row

    def ledger(self, artifact: Path, row: dict) -> Path:
        rows = [copy.deepcopy(row)]
        document = {
            "schema": "oxidex_ifd_identity_ledger_v1",
            "instrument": "synthetic narrow-row test fixture",
            "exiftool_version": "13.59",
            "source": {
                "tables_json_sha256": hashlib.sha256(self.source.read_bytes()).hexdigest(),
                "expr_ledger_sha256": "1" * 64,
                "ifd_rust_hash_format": codegen.IFD_RUST_HASH_FORMAT,
                "ifd_rust_sha256": codegen._canonical_ifd_rust_sha256(
                    table_modules.read_files(artifact.with_name("mod.rs"))
                ),
            },
            "counts": counts(rows),
            "rows": rows,
        }
        path = self.root / "ledger.json"
        path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
        return path

    def command(self, artifact: Path, ledger: Path, *extra: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [
                sys.executable,
                str(TOOL),
                str(self.source),
                "--module",
                "Olympus",
                "--table",
                "CameraSettings",
                "--tag-id",
                "0x0804",
                "--generated-row",
                str(artifact),
                "--identity-ledger",
                str(ledger),
                *extra,
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
        )

    def old_identity(self) -> dict:
        row = copy.deepcopy(self.identity)
        row["reader_state"] = "omitted"
        row["omissions"] = ["print_conv"]
        return row

    def test_producer_output_survives_rustfmt_then_check(self):
        artifact = self.artifact(omitted_row(self.literal))
        ledger = self.ledger(artifact, self.old_identity())
        produced = self.command(artifact, ledger)
        self.assertEqual(produced.returncode, 0, produced.stderr)
        subprocess.run(
            [
                "rustfmt",
                "--edition",
                "2024",
                "--config-path",
                str(ROOT / "rustfmt.toml"),
                str(artifact.with_name("mod.rs")),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        checked = self.command(artifact, ledger, "--check")
        self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_check_rejects_tampered_classification_digest_count_and_artifact_binding(self):
        for mutation in ("classification", "source_digest", "count", "artifact_digest"):
            with self.subTest(mutation=mutation):
                artifact = self.artifact(self.literal)
                ledger = self.ledger(artifact, self.identity)
                document = json.loads(ledger.read_text())
                if mutation == "classification":
                    document["rows"][0]["reader_state"] = "omitted"
                    document["rows"][0]["omissions"] = ["print_conv"]
                elif mutation == "source_digest":
                    document["rows"][0]["source_sha256"] = "0" * 64
                elif mutation == "count":
                    document["counts"]["reader_eligible"] = 0
                else:
                    document["source"]["ifd_rust_sha256"] = "0" * 64
                ledger.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
                before_artifact = artifact.read_bytes()
                before_ledger = ledger.read_bytes()
                checked = self.command(artifact, ledger, "--check")
                self.assertNotEqual(checked.returncode, 0, checked.stdout)
                self.assertEqual(artifact.read_bytes(), before_artifact)
                self.assertEqual(ledger.read_bytes(), before_ledger)

    def test_invalid_source_identity_cannot_partially_replace_generated_artifact(self):
        for mutation in ("source_digest", "variant_path"):
            with self.subTest(mutation=mutation):
                artifact = self.artifact(omitted_row(self.literal))
                ledger = self.ledger(artifact, self.old_identity())
                document = json.loads(ledger.read_text())
                if mutation == "source_digest":
                    document["rows"][0]["source_sha256"] = "0" * 64
                else:
                    document["rows"][0]["variant_path"] = [0]
                ledger.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
                before_artifact = artifact.read_bytes()
                before_ledger = ledger.read_bytes()
                result = self.command(artifact, ledger)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(artifact.read_bytes(), before_artifact)
                self.assertEqual(ledger.read_bytes(), before_ledger)

    def test_check_rejects_changed_generated_string_even_with_matching_artifact_digest(self):
        artifact = self.artifact(self.literal)
        ledger = self.ledger(artifact, self.identity)
        artifact.write_text(artifact.read_text().replace("Live Composite", "Live Compositf"))
        document = json.loads(ledger.read_text())
        document["source"]["ifd_rust_sha256"] = codegen._canonical_ifd_rust_sha256(
            table_modules.read_files(artifact.with_name("mod.rs"))
        )
        ledger.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
        before_artifact = artifact.read_bytes()
        before_ledger = ledger.read_bytes()
        result = self.command(artifact, ledger, "--check")
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("does not match fresh source output", result.stderr)
        self.assertEqual(artifact.read_bytes(), before_artifact)
        self.assertEqual(ledger.read_bytes(), before_ledger)

    def test_update_repairs_stale_aggregate_counts_and_artifact_binding(self):
        artifact = self.artifact(self.literal)
        ledger = self.ledger(artifact, self.identity)
        document = json.loads(ledger.read_text())
        document["counts"]["reader_eligible"] = 0
        document["counts"]["reader_omitted"] = 1
        document["source"]["ifd_rust_sha256"] = "0" * 64
        ledger.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
        artifact_mode = artifact.stat().st_mode & 0o7777
        ledger_mode = ledger.stat().st_mode & 0o7777
        result = self.command(artifact, ledger)
        self.assertEqual(result.returncode, 0, result.stderr)
        repaired = json.loads(ledger.read_text())
        self.assertEqual(artifact.stat().st_mode & 0o7777, artifact_mode)
        self.assertEqual(ledger.stat().st_mode & 0o7777, ledger_mode)
        self.assertEqual(repaired["rows"], [self.identity])
        self.assertEqual(repaired["counts"], counts([self.identity]))
        self.assertEqual(
            repaired["source"]["ifd_rust_sha256"],
            codegen._canonical_ifd_rust_sha256(
                table_modules.read_files(artifact.with_name("mod.rs"))
            ),
        )


if __name__ == "__main__":
    unittest.main()
