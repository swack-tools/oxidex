import json
import hashlib
import os
import shutil
import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DUMP = ROOT / "tools/exiftool-tables/dump_tables.pl"


def configured_perl() -> str:
    return os.environ.get("EXIFTOOL_PERL", "perl")


def configured_library() -> Path | None:
    configured = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
    if not configured:
        return None
    source = Path(configured)
    return source / "lib" if (source / "lib").is_dir() else source


def executable(command: str) -> bool:
    return Path(command).is_file() if "/" in command else shutil.which(command) is not None


PERL = configured_perl()
LIBRARY = configured_library()
NATIVE_READY = executable(PERL) and LIBRARY is not None and LIBRARY.is_dir()
SELECTED = (
    "Image::ExifTool::QuickTime::ItemList",
    "Image::ExifTool::XMP::SVG",
    "Image::ExifTool::JFIF::Main",
    "Image::ExifTool::Extra",
    "Image::ExifTool::Composite",
)


def dump(*args: str) -> dict:
    environment = {
        key: value for key, value in os.environ.items()
        if key not in {"PERL5LIB", "PERLLIB", "PERL5OPT"}
    }
    result = subprocess.run(
        [PERL, str(DUMP), "--reader-only", *args, str(LIBRARY), "QuickTime", "XMP", "JFIF"],
        check=True,
        text=True,
        capture_output=True,
        env=environment,
        timeout=30,
    )
    return json.loads(result.stdout)


@unittest.skipUnless(NATIVE_READY, "configured pinned ExifTool is unavailable")
class HydratedLayoutProjectionTests(unittest.TestCase):
    def test_hydrated_load_precedes_writer_and_final_contract_captures(self):
        source = DUMP.read_text()
        hydrated = source.index("my $hydrated_layouts = $HYDRATED_LAYOUTS")
        writer_capture = source.index("my ($write_autoload_router_status")
        reader_contract = source.index("NativeReaderContract::finalise_loaded_contract")
        utf8_contract = source.index("Utf8PrimitiveContract::capture_final")
        self.assertLess(hydrated, writer_capture)
        self.assertLess(hydrated, reader_contract)
        self.assertLess(hydrated, utf8_contract)

    def test_opt_in_projection_preserves_legacy_modules_and_serializes_full_names(self):
        legacy = dump()
        projected = dump(
            "--hydrated-layouts",
            *sum((["--hydrated-layout-table", name] for name in SELECTED), []),
        )
        self.assertNotIn("hydrated_layouts", legacy)
        self.assertEqual(projected["modules"], legacy["modules"])

        layouts = projected["hydrated_layouts"]
        self.assertEqual(layouts["schema"], "oxidex_hydrated_layout_projection_v1")
        provenance = layouts["source_provenance"]
        self.assertEqual(provenance["producer_sha256"], hashlib.sha256(DUMP.read_bytes()).hexdigest())
        for name in ["Image/ExifTool.pm", "Image/ExifTool/QuickTime.pm", "Image/ExifTool/BuildTagLookup.pm"]:
            self.assertEqual(provenance["sources"][name], {
                "library_relative_path": name,
                "sha256": hashlib.sha256((LIBRARY / name).read_bytes()).hexdigest(),
            })
        self.assertEqual(layouts["selection"], "explicit_full_name_subset")
        self.assertGreaterEqual(layouts["available_table_count"], 1512)
        self.assertEqual(layouts["requested_table_count"], len(SELECTED))
        self.assertEqual(layouts["table_count"], len(SELECTED))
        self.assertEqual(set(layouts["tables"]), set(SELECTED))
        self.assertEqual(layouts["helpers"]["shortcuts"], {
            "full_name": "Image::ExifTool::Shortcuts::Main",
            "kind": "shortcut_macro_table",
            "entry_count": 11,
        })

        quicktime = layouts["tables"]["Image::ExifTool::QuickTime::ItemList"]
        self.assertEqual(quicktime["kind"], "hydrated_table")
        self.assertEqual(quicktime["tag_count"], len(quicktime["tags"]))
        self.assertIn("\u00a9nam", quicktime["tags"])
        self.assertEqual(
            quicktime["tags"]["cpil"]["PrintConv"]["kind"], "enum"
        )
        self.assertTrue(quicktime["meta"]["PROCESS_PROC"]["resolved"])
        self.assertEqual(quicktime["meta"]["FORMAT"], "string")

        for full_name, kind in (
            ("Image::ExifTool::XMP::SVG", "hydrated_table"),
            ("Image::ExifTool::JFIF::Main", "hydrated_table"),
            ("Image::ExifTool::Extra", "extra_generated"),
            ("Image::ExifTool::Composite", "composite_aggregate"),
        ):
            table = layouts["tables"][full_name]
            self.assertEqual(table["kind"], kind)
            self.assertEqual(table["tag_count"], len(table["tags"]))

    def test_hydrated_runtime_references_are_interned_with_joinable_identity(self):
        projected = dump(
            "--hydrated-layouts",
            "--hydrated-layout-table", "Image::ExifTool::Exif::Main",
        )
        layouts = projected["hydrated_layouts"]
        objects = layouts["shared_reference_objects"]
        self.assertTrue(objects)
        object_references = []

        def walk(value):
            if isinstance(value, dict):
                if "object_id" in value:
                    object_references.append(value["object_id"])
                for item in value.values():
                    walk(item)
            elif isinstance(value, list):
                for item in value:
                    walk(item)

        walk(layouts["tables"])
        self.assertTrue(object_references)
        self.assertTrue(all(object_id in objects for object_id in object_references))
        self.assertTrue(all(object["kind"] in {"HASH", "ARRAY", "SCALAR"}
                            for object in objects.values()))
        interop = layouts["tables"]["Image::ExifTool::Exif::Main"]["tags"]["1"]
        self.assertEqual(interop["Name"], "InteropIndex")
        self.assertEqual(interop["TagID"], "1")
        self.assertEqual(interop["Table"], {
            "__ref": "tag_table",
            "table_full_names": ["Image::ExifTool::Exif::Main"],
        })

    def test_native_special_tag_filter_retains_underscore_raw_keys(self):
        projected = dump(
            "--hydrated-layouts",
            "--hydrated-layout-table", "Image::ExifTool::QuickTime::UserData",
            "--hydrated-layout-table", "Image::ExifTool::ZIP::Main",
        )
        tables = projected["hydrated_layouts"]["tables"]
        self.assertIn("_cx_", tables["Image::ExifTool::QuickTime::UserData"]["tags"])
        self.assertIn("_com", tables["Image::ExifTool::ZIP::Main"]["tags"])

    def test_hydrated_layout_filter_refuses_an_identity_outside_all_tables(self):
        environment = {
            key: value for key, value in os.environ.items()
            if key not in {"PERL5LIB", "PERLLIB", "PERL5OPT"}
        }
        result = subprocess.run(
            [PERL, str(DUMP), "--reader-only", "--hydrated-layouts",
             "--hydrated-layout-table", "Image::ExifTool::NotReal::Main", str(LIBRARY), "Exif"],
            text=True,
            capture_output=True,
            env=environment,
            timeout=30,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a catalog identity", result.stderr)


if __name__ == "__main__":
    unittest.main()
