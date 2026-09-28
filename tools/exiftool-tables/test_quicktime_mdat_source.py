"""Selected Main mdat names captured from native 11.78, 12.64 and 13.59."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import quicktime_protocol_caps as caps
import quicktime_generated_specs as itemlist
import quicktime_keys_specs as keys
import quicktime_userdata_specs as userdata

HERE = Path(__file__).resolve().parent


class MainMdatSource(unittest.TestCase):
    def test_selected_native_declarations_drive_generated_operands(self):
        for version, names in (("11.78", ("MovieDataSize", "MovieDataOffset")),
                               ("12.64", ("MediaDataSize", "MediaDataOffset")),
                               ("13.59", ("MediaDataSize", "MediaDataOffset"))):
            with self.subTest(version=version), tempfile.TemporaryDirectory() as directory:
                document = json.loads((HERE / "fixtures" /
                    f"quicktime_source_{version.replace('.', '_')}.json").read_text())
                root = Path(directory)
                (root / ".exiftool-version").write_text(version + "\n")
                with patch.object(caps, "ROOT", root), patch.object(itemlist, "ROOT", root), \
                     patch.object(keys, "ROOT", root), patch.object(userdata, "ROOT", root):
                    result = caps.compile_document(document)
                self.assertEqual(caps.main_mdat_names(document), names)
                rendered = caps.render_rust(result)
                for operand, name in zip(("SIZE", "OFFSET"), names):
                    self.assertIn(f'const MDAT_{operand}_TAG: &str = "QuickTime:{name}";', rendered)
                if version == "13.59":
                    self.assertEqual(rendered, caps.RUST.read_text())

    def test_unknown_controls_and_mixed_source_names_fail_closed(self):
        document = json.loads((HERE / "fixtures/quicktime_source_11_78.json").read_text())
        document["quicktime_main_mdat_tags"]["mdat-size"]["RawConv"] = "$val + 1"
        with self.assertRaisesRegex(ValueError, "unreviewed.*conversion"):
            caps.main_mdat_names(document)
        document = json.loads((HERE / "fixtures/quicktime_source_12_64.json").read_text())
        document["quicktime_main_mdat_tags"]["mdat-offset"]["RawConv"] = "$val + 1"
        with self.assertRaisesRegex(ValueError, "unreviewed.*conversion"):
            caps.main_mdat_names(document)

    def test_normal_full_dump_rows_produce_bounded_native_fact(self):
        for version in ("11.78", "12.64", "13.59"):
            document = json.loads((HERE / "fixtures" /
                f"quicktime_source_{version.replace('.', '_')}.json").read_text())
            native = document.pop("quicktime_main_mdat_tags")
            legacy = {}
            for key, value in native.items():
                if isinstance(value, str):
                    legacy[key] = {"Name": value, "_shorthand": True}
                else:
                    legacy[key] = dict(value)
                    if "RawConv" in legacy[key]:
                        legacy[key]["RawConv"] = {"kind": "expr", "expr": legacy[key]["RawConv"]}
            document["modules"]["QuickTime"]["tables"]["Main"] = {"tags": legacy}
            self.assertEqual(caps.selected_main_mdat_rows(document), native)
            self.assertEqual(caps.main_mdat_names(document),
                             (native["mdat-size"]["Name"], native["mdat-offset"]
                              if isinstance(native["mdat-offset"], str) else native["mdat-offset"]["Name"]))
            legacy["mdat-size"]["_shorthand"] = False
            with self.assertRaisesRegex(ValueError, "unreviewed.*shorthand"):
                caps.selected_main_mdat_rows(document)


if __name__ == "__main__":
    unittest.main()
