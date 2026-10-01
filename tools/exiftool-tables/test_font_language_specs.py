"""Font.pm's runtime language lookup remains a separate, complete lookup artifact."""
from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import font_language_specs as specs

HERE = Path(__file__).resolve().parent


class FontLanguageSpecTests(unittest.TestCase):
    def setUp(self):
        self.capture = json.loads(specs.FIXTURE.read_text(encoding="utf-8"))

    def test_pinned_lookup_is_complete_and_platform_separated(self):
        specs.validate(self.capture)
        p = self.capture["platforms"]
        if specs.PIN == "13.59":
            self.assertEqual([len(p[name]) for name in specs.PLATFORMS], [0, 0, 117, 0, 210])
            self.assertEqual(p["Windows"]["1044"], "no-NO")
            self.assertEqual(p["Macintosh"]["9"], "no")
            self.assertEqual(p["Macintosh"]["0"], "en")
            self.assertNotIn("9", p["Windows"])
        self.assertEqual(specs.render(self.capture), specs.RUST.read_text(encoding="utf-8"))

    def test_rejects_malformed_lookup_without_guessing(self):
        mutations = [
            lambda c: c.update(extra=1),
            lambda c: c["platforms"].pop("Windows"),
            lambda c: c["platforms"]["Windows"].update({"9": "en"}),
            lambda c: c["platforms"]["Windows"].update({"1044": "no"}),
            lambda c: c["platforms"]["Macintosh"].update({"9": 'n"o'}),
            lambda c: c["platforms"]["Windows"].update({"65536": "x"}),
            lambda c: c["platforms"]["Windows"].update({"01044": "no-NO"}),
            lambda c: c.update(source_sha256="not-a-hash"),
        ]
        for mutate in mutations:
            broken = copy.deepcopy(self.capture)
            mutate(broken)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                specs.validate(broken)

    def test_new_pin_renders_changed_runtime_lookup(self):
        changed = copy.deepcopy(self.capture)
        changed["exiftool_version"] = "13.60"
        changed["source_sha256"] = "a" * 64
        changed["platforms"]["Windows"]["1044"] = "nb-NO"
        changed["platforms"]["Windows"]["9"] = "en"
        with patch.object(specs, "PIN", "13.60"):
            rendered = specs.render(changed)
        self.assertIn('(1044, "nb-NO")', rendered)
        self.assertIn('(9, "en")', rendered)
        self.assertNotEqual(rendered, specs.RUST.read_text(encoding="utf-8"))

    def test_fresh_capture_matches_committed_fixture(self):
        perl = os.environ.get("EXIFTOOL_PERL")
        source = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
        if not perl or not source:
            if os.environ.get("GITHUB_ACTIONS"):
                self.fail("fresh pinned Font.pm source inputs missing in CI")
            self.skipTest("set EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
        library = Path(source) / "lib" if (Path(source) / "lib").is_dir() else Path(source)
        env = {key: value for key, value in os.environ.items() if not key.startswith("PERL5")}
        run = subprocess.run([perl, str(HERE / "capture_font_languages.pl"), str(library)],
                             capture_output=True, text=True, env=env, check=False)
        self.assertEqual(run.returncode, 0, run.stderr)
        fresh = json.loads(run.stdout)
        self.assertEqual(fresh, self.capture)
        self.assertEqual(fresh["source_sha256"], hashlib.sha256(
            (library / "Image/ExifTool/Font.pm").read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
