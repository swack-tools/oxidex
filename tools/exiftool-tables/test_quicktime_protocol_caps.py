"""Source-selected QuickTime reader branches, including historical releases."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import quicktime_generated_specs as itemlist
import quicktime_keys_specs as keys
import quicktime_userdata_specs as userdata
import quicktime_protocol_caps as caps

HERE = Path(__file__).resolve().parent


def source(version):
    return json.loads((HERE / "fixtures" / f"quicktime_source_{version.replace('.', '_')}.json").read_text())


class ProtocolCapabilities(unittest.TestCase):
    def compile(self, version, document):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".exiftool-version").write_text(version + "\n")
            with (patch.object(caps, "ROOT", root), patch.object(itemlist, "ROOT", root),
                  patch.object(keys, "ROOT", root), patch.object(userdata, "ROOT", root)):
                return caps.compile_document(document)

    def test_old_and_new_source_select_distinct_reader_branches(self):
        old = self.compile("11.78", source("11.78"))
        mid = self.compile("12.64", source("12.64"))
        current = self.compile("13.59", source("13.59"))
        self.assertEqual(old["generated_counts"], {"ItemList": 73, "Keys": 48, "UserData": 17})
        self.assertEqual(mid["generated_counts"], {"ItemList": 89, "Keys": 53, "UserData": 17})
        self.assertEqual(current["generated_counts"], {"ItemList": 92, "Keys": 70, "UserData": 17})
        self.assertEqual(old["capabilities"], {
            "implicit_int64": False, "shorten_explicit_width": False,
            "mdta_strip_generic_com": False, "mdta_retry_full_key": False,
            "userdata_xmp_isutf8": True})
        for modern in (mid, current):
            self.assertEqual(modern["capabilities"], {
                "implicit_int64": True, "shorten_explicit_width": True,
                "mdta_strip_generic_com": True, "mdta_retry_full_key": True,
                "userdata_xmp_isutf8": False})
        self.assertIn("pub(crate) const IMPLICIT_INT64: bool = false;", caps.render_rust(old))
        self.assertEqual(caps.render_rust(current), caps.RUST.read_text())

    def test_capability_provenance_is_selected_body_not_release_string(self):
        for version in ("11.78", "12.64", "13.59"):
            document = source(version)
            actual = self.compile(version, document)
            dependencies = document["quicktime_itemlist_reader_protocol"]["dependencies"]
            body = document["modules"]["QuickTime"]["tables"]["Keys"]["meta"]["PROCESS_PROC"]["__deparse"]
            self.assertEqual(actual["source"]["quicktime_format_sha256"],
                             hashlib.sha256(dependencies["quicktime_format"]["__deparse"].encode()).hexdigest())
            self.assertEqual(actual["source"]["process_keys_sha256"], hashlib.sha256(body.encode()).hexdigest())
        document = source("11.78")
        document["quicktime_userdata_reader_protocol"]["dependencies"]["Image::ExifTool::XMP::IsUTF8"]["__deparse"] += "\n# changed"
        with self.assertRaisesRegex(ValueError, "UserData:.*unsupported_userdata_helper"):
            self.compile("11.78", document)

    def test_unreviewed_source_combination_and_empty_table_fail_closed(self):
        document = source("12.64")
        document["modules"]["QuickTime"]["tables"]["Keys"]["meta"]["PROCESS_PROC"]["__deparse"] += "\n# changed"
        with self.assertRaisesRegex(ValueError, "Keys:.*source rows but no generated specs"):
            self.compile("12.64", document)
        document = source("11.78")
        document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]["__deparse"] += "\n# changed"
        with self.assertRaisesRegex(ValueError, "ItemList:.*source rows but no generated specs"):
            self.compile("11.78", document)


if __name__ == "__main__":
    unittest.main()
