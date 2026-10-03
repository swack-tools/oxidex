"""Historical PLUS source versions must remain pinned and fail closed."""

from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_plus_media_matrix as producer  # noqa: E402


class PlusMediaMatrixVersions(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.rows = [[f"{i // (26 ** 3)}{chr(65 + i // (26 ** 2) % 26)}"
                      f"{chr(65 + i // 26 % 26)}{chr(65 + i % 26)}", "description"]
                     for i in range(2143)]

    def source(self, exiftool_version: str, module_version: str) -> dict:
        return {
            "schema": "plus_media_matrix_v1",
            "exiftool_version": exiftool_version,
            "module": "Image/ExifTool/PLUS.pm",
            "module_version": module_version,
            "exceptions": {"OTHER": "CODE", "Notes": "STRING"},
            "source_sha256": "a" * 64,
            "rows": self.rows,
        }

    def render(self, pin: str, data: dict) -> str:
        (self.root / ".exiftool-version").write_text(pin + "\n")
        with patch.object(producer, "ROOT", self.root), patch.object(
            producer.subprocess, "run", side_effect=lambda _, **kwargs: type(
                "Result", (), {"stdout": kwargs["input"]})()
        ):
            return producer.render(data)

    def test_selected_historical_and_current_versions(self):
        for pin, module in (("11.78", "1.00"), ("12.64", "1.00"),
                            ("13.59", "1.02")):
            with self.subTest(pin=pin):
                output = self.render(pin, self.source(pin, module))
                self.assertIn(f"ExifTool {pin}, PLUS.pm {module}", output)
                self.assertIn('("0AAA", "description")', output)
                self.assertIn('pub static ROWS: [(&str, &str); 2143]', output)

    def test_mismatched_or_unknown_source_version_is_refused(self):
        for pin, module in (("11.78", "1.02"), ("12.64", "1.02"),
                            ("13.59", "1.00"), ("13.60", "1.02"), ("13.60", None)):
            with self.subTest(pin=pin, module=module):
                with self.assertRaisesRegex(ValueError, "source contract changed"):
                    self.render(pin, self.source(pin, module))


if __name__ == "__main__":
    unittest.main()
