"""Pinned Exif::Main 0xa301 inverse facts, without release-string dispatch."""

import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import scene_type_inverse_codegen as generator

HERE = Path(__file__).resolve().parent
FIXTURE = json.loads((HERE / "fixtures/scene_type_inverse_sources.json").read_text())


def document(version):
    row = copy.deepcopy(FIXTURE["records"][version]["row"])
    return {"exiftool_version": version,
            "modules": {"Exif": {"tables": {"Main": {"tags": {"41729": row}}}}}}


class SceneTypeInverseTests(unittest.TestCase):
    def compile(self, version, source):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".exiftool-version").write_text(version + "\n")
            with patch.object(generator, "ROOT", root):
                return generator.compile_document(source)

    def test_selected_source_recipe_controls_mask(self):
        expected = {"11.78": ("chr($val)", "unmasked_chr", "false"),
                    "12.64": ("chr($val & 0xff)", "masked_chr", "true"),
                    "13.59": ("chr($val & 0xff)", "masked_chr", "true")}
        for version, (expr, mode, rust_bool) in expected.items():
            with self.subTest(version=version):
                source = document(version)
                result = self.compile(version, source)
                row = source["modules"]["Exif"]["tables"]["Main"]["tags"]["41729"]
                self.assertEqual((result["source_value_conv_inv"], result["mode"]), (expr, mode))
                self.assertEqual(result["source_row_sha256"], hashlib.sha256(json.dumps(
                    row, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest())
                self.assertIn(f"SCENE_TYPE_MASKS_TO_BYTE: bool = {rust_bool};", generator.render_rust(result))
        pin = (generator.ROOT / ".exiftool-version").read_text().strip()
        selected = self.compile(pin, document(pin))
        self.assertEqual(generator.render_rust(selected), generator.RUST.read_text())
        self.assertEqual(json.dumps(selected, sort_keys=True, indent=2) + "\n", generator.LEDGER.read_text())

    def test_unreviewed_recipe_row_or_pin_fails_closed(self):
        source = document("11.78")
        row = source["modules"]["Exif"]["tables"]["Main"]["tags"]["41729"]
        row["ValueConvInv"]["expr"] = "chr($val % 256)"
        with self.assertRaisesRegex(ValueError, "unreviewed.*ValueConvInv"):
            self.compile("11.78", source)
        source = document("12.64")
        source["modules"]["Exif"]["tables"]["Main"]["tags"]["41729"]["Writable"] = "int8u"
        with self.assertRaisesRegex(ValueError, "unreviewed.*declaration"):
            self.compile("12.64", source)
        source = document("11.78")
        del source["modules"]["Exif"]["tables"]["Main"]["tags"]["41729"]
        with self.assertRaisesRegex(ValueError, "row is absent"):
            self.compile("11.78", source)
        with self.assertRaisesRegex(ValueError, "source differs from repository pin"):
            self.compile("13.59", document("12.64"))


if __name__ == "__main__":
    unittest.main()
