"""Pinned-native evidence test for the direct CheckExif fixture producer."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


HERE = Path(__file__).resolve().parent
PRODUCER = HERE / "checkexif_native_differential.py"
PERL = os.environ.get("EXIFTOOL_PERL")
LIBRARY = os.environ.get("OXIDEX_PINNED_EXIFTOOL")


class CheckExifNativeDifferentialTests(unittest.TestCase):
    def test_native_paths_are_required_without_environment(self):
        environment = {key: value for key, value in os.environ.items()
                       if key not in {"EXIFTOOL_PERL", "OXIDEX_PINNED_EXIFTOOL"}}
        result = subprocess.run(
            [sys.executable, str(PRODUCER), "--evidence-root", "unused"],
            env=environment, capture_output=True, text=True, timeout=15,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--perl/--lib are required", result.stderr)

    @unittest.skipUnless(PERL and LIBRARY, "requires gate-provided EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
    def test_pinned_native_fixture_authenticates_and_detects_source_change(self):
        with tempfile.TemporaryDirectory(prefix="oxidex-checkexif-evidence-") as temporary:
            root = Path(temporary)
            subprocess.run(
                [sys.executable, str(PRODUCER), "--perl", PERL, "--lib", LIBRARY,
                 "--evidence-root", str(root)],
                check=True,
                capture_output=True,
                text=True,
                timeout=45,
            )
            canonical = json.loads((root / "canonical" / "checkexif-native.json").read_text())
            changed = json.loads((root / "changed" / "checkexif-native.json").read_text())
            cases = json.loads((root / "canonical" / "checkexif-cases.json").read_text())
            self.assertEqual([item["name"] for item in cases], [item["name"] for item in canonical["results"]])
            self.assertNotEqual(canonical["results"], changed["results"])
            self.assertNotEqual(canonical["body_sha256"], changed["body_sha256"])
            for label, payload in (("canonical", canonical), ("changed", changed)):
                copied = (root / label / "WriteExif.pl").read_bytes()
                self.assertEqual(hashlib.sha256(copied).hexdigest(), payload["source_sha256"])
                check_value = payload["check_value"]
                self.assertEqual(
                    hashlib.sha256((root / label / "Writer.pl").read_bytes()).hexdigest(),
                    check_value["source_sha256"],
                )
                scope = json.loads((root / label / "scope.json").read_text())
                self.assertIn("direct", scope["scope"])
                self.assertIn("not_covered", scope)

    @unittest.skipUnless(PERL and LIBRARY, "requires gate-provided EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
    def test_bare_perl_name_resolves_only_in_controlled_path(self):
        resolved = shutil.which(PERL, path=os.environ.get("PATH"))
        self.assertIsNotNone(resolved)
        perl = Path(resolved).resolve()
        environment = dict(os.environ)
        environment.pop("EXIFTOOL_PERL", None)
        environment.pop("OXIDEX_PINNED_EXIFTOOL", None)
        environment["PATH"] = str(perl.parent)
        with tempfile.TemporaryDirectory(prefix="oxidex-checkexif-bare-perl-") as temporary:
            subprocess.run(
                [sys.executable, str(PRODUCER), "--perl", perl.name, "--lib", LIBRARY,
                 "--evidence-root", temporary],
                check=True, capture_output=True, text=True, timeout=45, env=environment,
            )


if __name__ == "__main__":
    unittest.main()
