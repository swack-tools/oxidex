import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hydrated_catalog_reconcile as reconcile


PRODUCER = HERE / "dump_hydrated_catalog.pl"
LOCAL_PERL = "/tmp/oxidex-perl538-build-20260913-r2/prefix/bin/perl5.38.2"
LOCAL_LIBRARY = Path("/tmp/oxidex-exiftool-cache/exiftool/lib")


def configured_perl() -> str:
    return os.environ.get("EXIFTOOL_PERL", LOCAL_PERL)


def configured_library() -> Path:
    source = Path(os.environ.get("OXIDEX_PINNED_EXIFTOOL", str(LOCAL_LIBRARY)))
    # CI exports the ExifTool source root; local development commonly names lib.
    return source / "lib" if (source / "lib").is_dir() else source


def executable(command: str) -> bool:
    return Path(command).is_file() if "/" in command else shutil.which(command) is not None


CANONICAL_PERL = configured_perl()
CANONICAL_LIB = configured_library()
NATIVE_READY = executable(CANONICAL_PERL) and CANONICAL_LIB.is_dir()


def table(module: str, name: str) -> dict:
    return {"full_name": f"Image::ExifTool::{module}::{name}", "meta": {}, "tags": {"1": {}}, "tag_count": 1}


def dump() -> dict:
    return {"exiftool_version": "13.59", "modules_ok": 1, "modules_failed": 0,
            "modules": {"Exif": {"module": "Exif", "table_count": 1, "tables": {"Main": table("Exif", "Main")}}}}


def catalog() -> dict:
    return {"schema": reconcile.CATALOG_SCHEMA, "exiftool_version": "13.59",
            "counts": {"hydrated_tables": 2},
            "families": {"hydrated_tables": [
                {"full_name": "Image::ExifTool::Exif::Main", "kind": "hydrated_table"},
                {"full_name": "Image::ExifTool::Extra", "kind": "extra_generated"}],
                         "shortcuts": [{"full_name": "Image::ExifTool::Shortcuts::Main", "kind": "shortcut_macro_table", "entry_count": 1}]}}


class HydratedCatalogUniverse(unittest.TestCase):
    def test_reconciliation_conserves_tables_and_helpers(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "dump.json"
            path.write_text(json.dumps(dump()), encoding="utf-8")
            report = reconcile.reconcile(catalog(), path, "13.59", catalog_sha256="a" * 64)
        rows = report["reconciliation"]
        self.assertEqual(rows["matched_tables"], 1)
        self.assertEqual(rows["missing_tables"], ["Image::ExifTool::Extra"])
        self.assertEqual(rows["matched_helpers"], 0)
        self.assertEqual(rows["extra_dump_identities"], [])
        self.assertEqual(rows["conservation"], {"catalog_tables": 2, "catalog_helpers": 1, "dump_identities": 1})

    def test_streaming_dump_reader_refuses_mismatched_table_identity(self):
        bad = dump()
        bad["modules"]["Exif"]["tables"]["Main"]["full_name"] = "Image::ExifTool::Other::Main"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "dump.json"
            path.write_text(json.dumps(bad), encoding="utf-8")
            with self.assertRaisesRegex(reconcile.Refused, "identity schema"):
                reconcile.stream_dump_identities(path, "13.59")

    def test_streaming_dump_reader_refuses_unconserved_tags(self):
        bad = dump()
        bad["modules"]["Exif"]["tables"]["Main"]["tag_count"] = 2
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "dump.json"
            path.write_text(json.dumps(bad), encoding="utf-8")
            with self.assertRaisesRegex(reconcile.Refused, "does not conserve"):
                reconcile.stream_dump_identities(path, "13.59")

    def test_streaming_reader_refuses_wrong_version_and_module_count(self):
        for mutate, error in (
            (lambda value: value.update({"exiftool_version": "13.58"}), "version"),
            (lambda value: value.update({"modules_ok": 2}), "module count"),
        ):
            bad = dump(); mutate(bad)
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "dump.json"
                path.write_text(json.dumps(bad), encoding="utf-8")
                with self.assertRaisesRegex(reconcile.Refused, error):
                    reconcile.stream_dump_identities(path, "13.59")

    def test_reader_refuses_duplicate_fields_and_invalid_skipped_escape(self):
        duplicate = ('{"exiftool_version":"13.59","exiftool_version":"13.59",'
                     '"modules_ok":1,"modules_failed":0,"modules":{}}')
        invalid_escape = ('{"exiftool_version":"13.59","modules_ok":1,"modules_failed":0,'
                          '"modules":{"Exif":{"module":"Exif","table_count":1,"tables":'
                          '{"Main":{"full_name":"Image::ExifTool::Exif::Main","meta":{"bad":"\\q"},'
                          '"tags":{},"tag_count":0}}}}}')
        for source, error in ((duplicate, "duplicate dump root"), (invalid_escape, "invalid escape")):
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "dump.json"
                path.write_text(source, encoding="utf-8")
                with self.assertRaisesRegex(reconcile.Refused, error):
                    reconcile.stream_dump_identities(path, "13.59")

    def test_catalog_refuses_wrong_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "dump.json"; path.write_text(json.dumps(dump()), encoding="utf-8")
            wrong = catalog(); wrong["exiftool_version"] = "13.58"
            with self.assertRaisesRegex(reconcile.Refused, "catalog ExifTool version"):
                reconcile.reconcile(wrong, path, "13.59")

    def test_cli_prints_instrument_header_and_refuses_dirty_repository(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory); root = base / "repo"
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            (root / ".exiftool-version").write_text("13.59\n", encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", ".exiftool-version"], check=True)
            subprocess.run(["git", "-C", str(root), "-c", "user.name=test", "-c", "user.email=test@example.test", "commit", "-qm", "pin"], check=True)
            catalog_path, dump_path, output = base / "catalog.json", base / "dump.json", base / "out.json"
            catalog_path.write_text(json.dumps(catalog()), encoding="utf-8")
            dump_path.write_text(json.dumps(dump()), encoding="utf-8")
            argv = [sys.executable, str(HERE / "hydrated_catalog_reconcile.py"), "--repo-root", str(root),
                    "--catalog", str(catalog_path), "--dump", str(dump_path), "--output", str(output)]
            refused_env = os.environ.copy()
            refused_env.pop("OXIDEX_ALLOW_DIRTY_TREE", None)
            clean = subprocess.run(argv, text=True, capture_output=True, env=refused_env)
            self.assertEqual(clean.returncode, 0, clean.stderr)
            self.assertIn("=== instrument: hydrated_catalog_reconcile.py ===", clean.stderr)
            (root / "dirty").write_text("x", encoding="utf-8")
            dirty = subprocess.run(argv[:-1] + [str(base / "dirty-output.json")], text=True, capture_output=True, env=refused_env)
        self.assertNotEqual(dirty.returncode, 0)
        self.assertIn("refusing to measure against a dirty working tree", dirty.stderr)

    @unittest.skipUnless(NATIVE_READY, "configured pinned ExifTool is unavailable")
    def test_pinned_producer_emits_hydrated_universe_and_separate_shortcuts(self):
        result = subprocess.run([CANONICAL_PERL, str(PRODUCER), str(CANONICAL_LIB)], check=True, text=True, capture_output=True,
                                env={key: value for key, value in os.environ.items() if key not in {"PERL5LIB", "PERLLIB", "PERL5OPT"}})
        document = json.loads(result.stdout)
        self.assertEqual(document["schema"], reconcile.CATALOG_SCHEMA)
        self.assertEqual(document["exiftool_version"], "13.59")
        self.assertEqual(document["counts"]["hydrated_tables"], 1512)
        self.assertEqual(document["counts"]["shortcut_entries"], 11)
        names = {row["full_name"] for row in document["families"]["hydrated_tables"]}
        self.assertIn("Image::ExifTool::Extra", names)
        self.assertIn("Image::ExifTool::Composite", names)
        self.assertEqual(document["families"]["shortcuts"][0]["full_name"], "Image::ExifTool::Shortcuts::Main")
        self.assertNotIn("selected_library", document["producer"])

    @unittest.skipUnless(NATIVE_READY, "configured pinned ExifTool is unavailable")
    def test_producer_refuses_a_repo_pin_that_differs_from_loaded_library(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".exiftool-version").write_text("13.58\n", encoding="utf-8")
            result = subprocess.run([CANONICAL_PERL, str(PRODUCER), "--repo-root", str(root), str(CANONICAL_LIB)], text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match repository pin", result.stderr)


if __name__ == "__main__":
    unittest.main()
