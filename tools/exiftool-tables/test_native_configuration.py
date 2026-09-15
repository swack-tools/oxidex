"""Native-test configuration must resolve CI's PATH Perl and checkout root."""
from __future__ import annotations

import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

from native_write_matrix import optional_native_configuration


class NativeConfigurationTests(unittest.TestCase):
    def test_path_perl_and_checkout_root_are_normalized(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl = root / "bin" / "perl"
            perl.parent.mkdir()
            perl.write_text("#!/bin/sh\n")
            perl.chmod(0o755)
            library = root / "exiftool" / "lib" / "Image" / "ExifTool"
            library.mkdir(parents=True)
            (library.parent / "ExifTool.pm").write_text("1;\n")
            (library / "Writer.pl").write_text("1;\n")
            with patch.dict("os.environ", {"PATH": str(perl.parent)}, clear=False):
                resolved = optional_native_configuration("perl", str(root / "exiftool"))
            self.assertEqual(resolved, (perl.resolve(), (root / "exiftool" / "lib").resolve()))

    def test_absent_is_skippable_but_partial_or_invalid_is_an_error(self):
        self.assertIsNone(optional_native_configuration(None, None))
        with self.assertRaisesRegex(ValueError, "together"):
            optional_native_configuration("perl", None)
        with self.assertRaisesRegex(ValueError, "not on PATH"):
            optional_native_configuration("not-a-real-perl", "/tmp")


if __name__ == "__main__":
    unittest.main()
