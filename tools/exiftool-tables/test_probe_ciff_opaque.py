"""Native source replay for the four currently omitted CanonRaw opaque scalars."""
import json
import os
from pathlib import Path
import subprocess
import unittest

PINNED = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
PERL = os.environ.get("EXIFTOOL_PERL")
CANONICAL_PERL = "v5.38.2"
PROBE = Path(__file__).with_name("probe_ciff_opaque.pl")


def request(order, entries, *, image_data_hash=False):
    return {
        "protocol": "oxidex.ciff_opaque.v1",
        "byte_order": order,
        "entries": entries,
        "image_data_hash": image_data_hash,
    }


def replay(requests):
    command = [PERL, str(PROBE), "--lib", "lib"]
    payload = "".join(json.dumps(item, separators=(",", ":")) + "\n" for item in requests)
    result = subprocess.run(command, cwd=PINNED, input=payload, text=True, capture_output=True,
                            timeout=15, check=True)
    if result.stderr:
        raise AssertionError(result.stderr)
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    if len(replies) != len(requests):
        raise AssertionError(f"expected {len(requests)} replies, got {len(replies)}")
    return replies


@unittest.skipUnless(PINNED and PERL, "set OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_PERL")
class CiffOpaqueNativeReplay(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        version = subprocess.check_output([PERL, "-e", "print $^V"], text=True).strip()
        if version != CANONICAL_PERL:
            raise AssertionError(f"opaque replay requires canonical Perl {CANONICAL_PERL}, got {version}")
        if not (Path(PINNED) / "lib" / "Image" / "ExifTool" / "CanonRaw.pm").is_file():
            raise AssertionError("selected pinned source does not contain CanonRaw.pm")

    def assert_source(self, reply):
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(reply["returned"]["numeric"], 1)
        process = reply["selection"]["process"]
        self.assertTrue(process["resolved"])
        self.assertEqual(process["name"], "Image::ExifTool::CanonRaw::ProcessCanonRaw")
        self.assertEqual(process["source_file"], "Image/ExifTool/CanonRaw.pm")
        validate = reply["selection"]["validate_image"]
        self.assertTrue(validate["resolved"])
        self.assertEqual(validate["name"], "Image::ExifTool::ValidateImage")
        self.assertEqual(validate["source_file"], "Image/ExifTool.pm")

    def test_inline_explicit_undef_is_one_byte_and_unformatted_is_eight(self):
        replies = replay([
            request(order, [
                {"tag": 0x4001, "inline_hex": "0102030405060708"},
                {"tag": 0x6005, "inline_hex": "1112131415161718"},
            ])
            for order in ("II", "MM")
        ])
        for reply in replies:
            self.assert_source(reply)
            self.assertEqual(reply["found"][0]["name"]["string"], "FreeBytes")
            self.assertEqual(reply["selection"]["rows"][0]["format"]["string"], "undef")
            self.assertEqual(reply["selection"]["rows"][0]["binary"]["numeric"], 1)
            self.assertEqual(reply["found"][0]["stored_bytes"],
                             {"defined": True, "length": 1, "hex": "01"})
            self.assertEqual(reply["found"][1]["name"]["string"], "RawData")
            self.assertEqual(reply["found"][1]["stored_bytes"],
                             {"defined": True, "length": 8, "hex": "1112131415161718"})

    def test_external_raw_and_preview_payloads_keep_full_bytes_and_repair_jpeg(self):
        replies = replay([
            request(order, [
                {"tag": 0x2005, "offset": 80, "payload_hex": "0001ff80"},
                {"tag": 0x2007, "offset": 96, "payload_hex": "00d8ffdb99"},
                {"tag": 0x2008, "offset": 112, "payload_hex": "ffd8ffdb22"},
            ])
            for order in ("II", "MM")
        ])
        for reply in replies:
            self.assert_source(reply)
            raw, jpg, thumb = reply["found"]
            self.assertFalse(reply["selection"]["rows"][0]["format"]["defined"])
            self.assertEqual(reply["selection"]["rows"][1]["raw_conv"]["string"],
                             "$self->ValidateImage(\\$val,$tag)")
            self.assertEqual(raw["stored_bytes"]["hex"], "0001ff80")
            self.assertEqual(jpg["group2"]["string"], "Preview")
            self.assertEqual(thumb["group2"]["string"], "Preview")
            self.assertEqual(jpg["stored_bytes"]["hex"], "ffd8ffdb99")
            self.assertEqual(thumb["stored_bytes"]["hex"], "ffd8ffdb22")

    def test_rawdata_hash_receives_absolute_external_span_before_reporting(self):
        replies = replay([
            request(order, [{"tag": 0x2005, "offset": 123, "payload_hex": "0123456789abcdef"}],
                    image_data_hash=True)
            for order in ("II", "MM")
        ])
        for reply in replies:
            self.assert_source(reply)
            self.assertEqual(reply["found"][0]["stored_bytes"]["hex"], "0123456789abcdef")
            self.assertEqual(reply["image_data_hash"], [{
                "offset": {"defined": True, "numeric": 123, "string": "123"},
                "size": {"defined": True, "numeric": 8, "string": "8"},
                "mode": {"defined": True, "string": "raw"},
            }])


if __name__ == "__main__":
    unittest.main()
