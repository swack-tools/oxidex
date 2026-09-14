"""Portable registry boundaries and official final-stage artifact freshness."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from final_scalar_stage import FinalStageRefused, _authenticate_registry, generate

ROOT = Path(__file__).resolve().parents[2]


def registry_document():
    return {"native_write_format_registry": {
        "state": "resolved",
        "source": {"library_relative_path": "Image/ExifTool/Exif.pm", "sha256": "a" * 64},
        "format_name": [None, "sample"],
        "format_size": [None, 1],
        "format_number": {"sample": 1, "alias": 1},
    }}


class FinalScalarCodegen(unittest.TestCase):
    def test_registry_preserves_aliases_without_duplicate_canonical_facts(self):
        registry = _authenticate_registry(registry_document())
        self.assertEqual(registry.canonical_facts, (("sample", 1, 1),))
        self.assertEqual(registry.aliases, (("alias", 1), ("sample", 1)))

    def test_boolean_and_out_of_range_numeric_facts_refuse(self):
        for field, key, value in (("format_size", 1, True), ("format_size", 1, 1 << 32),
                                  ("format_number", "alias", True),
                                  ("format_number", "alias", 1 << 16)):
            with self.subTest(field=field, value=value):
                document = registry_document()
                document["native_write_format_registry"][field][key] = value
                with self.assertRaises(FinalStageRefused):
                    _authenticate_registry(document)

    def test_unsupported_source_has_non_executable_artifacts_and_release_identity(self):
        source, report = generate({"exiftool_version": "test-release"})
        self.assertFalse(report["emitted"])
        self.assertTrue(report["reason"])
        self.assertEqual(report["recipes"], [])
        self.assertIsNone(report["registry"])
        self.assertIn("Option<NativeTiffFormatRegistry> = None;", source)
        self.assertEqual(report["exiftool_version"], "test-release")
        with self.assertRaises(TypeError):
            generate([])


@unittest.skipUnless(os.environ.get("OXIDEX_TABLES_JSON"), "requires fresh official native capture")
class FinalScalarFreshness(unittest.TestCase):
    def test_registered_artifacts_match_selected_source(self):
        source, report = generate(json.loads(Path(os.environ["OXIDEX_TABLES_JSON"]).read_text()))
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "rules.rs"
            path.write_text(source)
            subprocess.run(["rustfmt", "--edition", "2024", "--config-path", str(ROOT / "rustfmt.toml"), str(path)],
                           check=True, capture_output=True, timeout=30)
            self.assertEqual(path.read_text(), (ROOT / "src/writers/generated_tiff_scalar_final_rules.rs").read_text())
        self.assertEqual(json.dumps(report, sort_keys=True, indent=2) + "\n",
                         (ROOT / "tools/exiftool-tables/tiff_scalar_final_ledger.json").read_text())
