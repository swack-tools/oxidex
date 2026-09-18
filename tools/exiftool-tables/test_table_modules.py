"""The per-module artifact layout: render/write/read round-trips, the refusals,
and the property every text consumer relies on -- the logical text of a split
artifact carries the monolith's declarations in the monolith's order."""
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

import codegen
import table_modules

HEADER = "//! generated -- DO NOT EDIT\n"


def chunk(module, table, body):
    return codegen.TableChunk(f"\n/// `Image::ExifTool::{module}::{table}`\npub static {module.upper()}_{table.upper()}: u8 = {body};\n", module, table)


class StemTests(unittest.TestCase):
    def test_stem_rule(self):
        self.assertEqual(table_modules.module_stem("Canon"), "canon")
        self.assertEqual(table_modules.module_stem("ICC_Profile"), "icc_profile")
        self.assertEqual(table_modules.module_stem("H264"), "h264")
        self.assertEqual(table_modules.module_stem("Jpeg2000"), "jpeg2000")

    def test_unusable_stems_are_refused(self):
        for bad in ("", "3FR", "Mod", "Self", "Type", "Use"):
            with self.subTest(bad=bad), self.assertRaises(SystemExit):
                table_modules.module_stem(bad)


class RenderTests(unittest.TestCase):
    def render(self):
        chunks = [chunk("Nikon", "Main", "1"), chunk("Canon", "Main", "2"), chunk("Canon", "AFInfo", "3"),
                  chunk("ICC_Profile", "Main", "4")]
        return table_modules.render_files("//! hub -- DO NOT EDIT\n", "\npub static ALL: &[u8] = &[];\n",
                                          lambda m: f"//! {m} -- DO NOT EDIT\n", chunks), chunks

    def test_one_file_per_module_in_chunk_order_and_sorted_hub(self):
        files, chunks = self.render()
        self.assertEqual(sorted(files), ["canon.rs", "icc_profile.rs", "mod.rs", "nikon.rs"])
        # Chunk order within a module is the generator's; the hub's mod order is sorted.
        self.assertEqual(files["canon.rs"], "//! Canon -- DO NOT EDIT\n" + chunks[1] + chunks[2])
        self.assertEqual(
            table_modules.MOD_LINE_RE.findall(files["mod.rs"]), ["canon", "icc_profile", "nikon"])
        self.assertIn("pub use canon::*;\npub use icc_profile::*;\npub use nikon::*;\n", files["mod.rs"])
        self.assertTrue(files["mod.rs"].endswith("\npub static ALL: &[u8] = &[];\n"))

    def test_logical_text_is_the_monolith_order(self):
        files, chunks = self.render()
        logical = table_modules.logical_text(files)
        statics = re.findall(r"^pub static (\w+):", logical, re.M)
        self.assertEqual(statics, ["CANON_MAIN", "CANON_AFINFO", "ICC_PROFILE_MAIN", "NIKON_MAIN", "ALL"])
        self.assertTrue(logical.startswith("//! hub -- DO NOT EDIT\n"))
        self.assertNotIn("\nmod canon;\n", logical)

    def test_untagged_chunk_and_stem_collision_are_refused(self):
        with self.assertRaisesRegex(SystemExit, "no ExifTool module name"):
            table_modules.render_files("", "", lambda m: "", ["\npub static X: u8 = 1;\n"])
        with self.assertRaisesRegex(SystemExit, "collision"):
            table_modules.render_files("", "", lambda m: "", [chunk("Foo", "A", "1"), chunk("FOO", "B", "2")])


class WriteReadTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.hub = Path(self.tmp.name) / "binary" / "mod.rs"
        self.files, _ = RenderTests().render()

    def test_round_trip_and_stale_generated_sibling_removal(self):
        self.assertEqual(table_modules.write_files(self.hub, self.files), [])
        self.assertEqual(table_modules.read_files(self.hub), self.files)
        self.assertEqual(table_modules.read_logical(self.hub), table_modules.logical_text(self.files))
        self.assertEqual(table_modules.declared_stems(self.hub), ["canon", "icc_profile", "nikon"])
        smaller = {name: text for name, text in self.files.items() if name != "nikon.rs"}
        smaller["mod.rs"] = smaller["mod.rs"].replace("mod nikon;\n", "").replace("pub use nikon::*;\n", "")
        removed = table_modules.write_files(self.hub, smaller)
        self.assertEqual(removed, [self.hub.with_name("nikon.rs")])
        self.assertEqual(sorted(p.name for p in self.hub.parent.glob("*.rs")), ["canon.rs", "icc_profile.rs", "mod.rs"])

    def test_hand_written_sibling_is_never_deleted(self):
        table_modules.write_files(self.hub, self.files)
        stray = self.hub.with_name("hand_written.rs")
        stray.write_text("pub static HAND: u8 = 1;\n")
        with self.assertRaisesRegex(SystemExit, "not a generated table file"):
            table_modules.write_files(self.hub, self.files)
        self.assertTrue(stray.is_file())

    def test_hub_must_be_named_mod_rs(self):
        with self.assertRaisesRegex(SystemExit, "mod.rs"):
            table_modules.write_files(self.hub.with_name("binary_tables.rs"), self.files)

    def test_missing_declared_module_is_loud(self):
        table_modules.write_files(self.hub, self.files)
        self.hub.with_name("canon.rs").unlink()
        with self.assertRaisesRegex(SystemExit, "canon.rs is missing"):
            table_modules.read_files(self.hub)

    def test_single_file_path_reads_as_itself(self):
        legacy = Path(self.tmp.name) / "ifd_tables.rs"
        legacy.write_text("pub static IFD_X: u8 = 1;\n")
        self.assertEqual(table_modules.read_logical(legacy), "pub static IFD_X: u8 = 1;\n")
        self.assertEqual(table_modules.sibling_artifact(legacy, "ifd", "ifd_tables.rs"), legacy)
        self.assertEqual(table_modules.sibling_artifact(self.hub, "ifd", "ifd_tables.rs"),
                         self.hub.parent.parent / "ifd" / "mod.rs")
        self.assertEqual(table_modules.tables_dir(self.hub), self.hub.parent.parent)
        self.assertEqual(table_modules.tables_dir(legacy), legacy.parent)


class RustfmtOrderTests(unittest.TestCase):
    """The hub declares `mod`/`pub use` lines in the order rustfmt's
    reorder_modules/reorder_imports produce, so `format_artifacts` never
    reorders them and the spliced logical text equals what was rendered."""

    def test_hub_order_is_stable_under_rustfmt(self):
        import shutil
        if shutil.which("rustfmt") is None:
            self.skipTest("rustfmt not installed")
        import artifacts
        stems = list(artifacts.BINARY_MODULE_STEMS) + list(artifacts.IFD_MODULE_STEMS)
        with tempfile.TemporaryDirectory() as tmp:
            hub = Path(tmp) / "t" / "mod.rs"
            files = {f"{stem}.rs": "pub static X: u8 = 1;\n" for stem in set(stems)}
            files["mod.rs"] = table_modules.render_hub("", set(stems), "")
            table_modules.write_files(hub, files)
            before = hub.read_text()
            subprocess.run(["rustfmt", "--edition", "2024", str(hub)], check=True, capture_output=True)
            self.assertEqual(hub.read_text(), before)


if __name__ == "__main__":
    unittest.main()
