from __future__ import annotations

from dataclasses import replace
import os
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from checkexif_recipes import RecipeRefused
from final_scalar_stage import FinalScalarRecipe, NativeFormatRegistryArtifact
from setnewvalue_addressing import AddressRow, Addressing
from setnewvalue_public_migration_ledger import (
    build_from_current, compile_current, render_rust, validate_ledger,
    _current_from_authenticated, _digest,
)


def h(ch: str) -> str:
    return ch * 64


CAPTURE = {
    "exiftool_version": "13.59",
    "main_source_sha256": h("a"),
    "write_exif_source_sha256": h("b"),
    "writer_source_sha256": h("c"),
    "exif_source_sha256": h("d"),
}


def addressing(*rows: AddressRow) -> Addressing:
    return Addressing(tuple(rows), frozenset(row.name.lower() for row in rows),
                      "Image/ExifTool/TagLookup.pm", h("e"), h("f"),
                      "Image/ExifTool/Writer.pl", h("c"), h("0"),
                      h("1"), h("2"), {"opaque": "authenticated by production compiler"}, ((0, "EXIF"), (1, "IFD0")))


def row(name: str, raw_id: str, *, write_group: str = "IFD0") -> AddressRow:
    return AddressRow("Exif", "Main", "Image::ExifTool::Exif::Main", raw_id,
                      name, "EXIF", "IFD0", write_group)


def recipe(name: str, raw_id: int, *, write_group: str = "IFD0", control: str = h("3")) -> FinalScalarRecipe:
    return FinalScalarRecipe("Exif", "Main", "Image::ExifTool::Exif::Main", raw_id,
                             name, "EXIF", write_group, "int16u", "int16u", "CeilDivision", "DeleteEntry",
                             control, h("4"), h("b"), h("d"), h("c"), h("a"))


class PublicMigrationLedgerTest(unittest.TestCase):
    def current(self, rows, recipes, capture=CAPTURE):
        return _current_from_authenticated(addressing(*rows), recipes, capture)

    def test_intersection_only_migrates_composed_identity(self):
        source, current = self.current([row("Artist", "0x013b"), row("LegacyOnly", "0x013c")],
                                       [recipe("Artist", 0x013b)])
        ledger = build_from_current(source, current, None, bootstrap=True)
        self.assertEqual([(entry["name"], entry["state"]) for entry in ledger["entries"]],
                         [("Artist", "current")])
        self.assertNotIn("LegacyOnly", str(ledger))
        rendered = render_rust(ledger)
        self.assertIn('name: "Artist"', rendered)
        self.assertNotIn("LegacyOnly", rendered)
        self.assertIn("PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE", rendered)
        self.assertEqual(ledger["source"]["capture"], CAPTURE)

    def test_removed_or_unsupported_prior_identity_stays_terminal(self):
        source1, current1 = self.current([row("Artist", "315")], [recipe("Artist", 315)])
        prior = build_from_current(source1, current1, None, bootstrap=True)
        source2, current2 = self.current([row("Artist", "315")], [])
        upgraded = build_from_current(source2, current2, prior, bootstrap=False)
        entry = upgraded["entries"][0]
        self.assertEqual(entry["state"], "removed_or_unsupported")
        self.assertEqual([event["state"] for event in entry["history"]],
                         ["current", "removed_or_unsupported"])
        self.assertIn("removed_or_unsupported: true", render_rust(upgraded))

    def test_rename_keeps_old_identity_terminal_and_new_identity_current(self):
        source1, current1 = self.current([row("OldName", "315")], [recipe("OldName", 315)])
        prior = build_from_current(source1, current1, None, bootstrap=True)
        source2, current2 = self.current([row("NewName", "315")], [replace(recipe("NewName", 315), main_source_sha256=h("7"))],
                                         {**CAPTURE, "exiftool_version": "13.60", "main_source_sha256": h("7")})
        upgraded = build_from_current(source2, current2, prior, bootstrap=False)
        self.assertEqual([(entry["name"], entry["state"]) for entry in upgraded["entries"]],
                         [("NewName", "current"), ("OldName", "removed_or_unsupported")])

    def test_typechange_is_current_with_new_semantics_history(self):
        source1, current1 = self.current([row("Artist", "315")], [recipe("Artist", 315)])
        prior = build_from_current(source1, current1, None, bootstrap=True)
        changed = replace(recipe("Artist", 315, control=h("5")), main_source_sha256=h("6"))
        source2, current2 = self.current([row("Artist", "315")], [changed],
                                         {**CAPTURE, "exiftool_version": "13.60", "main_source_sha256": h("6")})
        upgraded = build_from_current(source2, current2, prior, bootstrap=False)
        entry = upgraded["entries"][0]
        self.assertEqual(entry["state"], "current")
        self.assertNotEqual(entry["history"][0]["semantics_sha256"], entry["history"][-1]["semantics_sha256"])

    def test_legacy_cohort_migrates_on_same_source_then_stays_frozen(self):
        source0, current0 = self.current([row("Old", "315")], [recipe("Old", 315)])
        prior0 = build_from_current(source0, current0, None, bootstrap=True)
        source1, current1 = self.current([row("Old", "315"), row("New", "316")],
                                         [replace(recipe("Old", 315), main_source_sha256=h("7")), replace(recipe("New", 316), main_source_sha256=h("7"))],
                                         {**CAPTURE, "main_source_sha256": h("7")})
        prior1 = build_from_current(source1, current1, prior0, bootstrap=False)
        # Model the committed pre-schema ledger: Old predates source1; New does not.
        legacy = dict(prior1); legacy.pop("predecessor_cohort"); legacy.pop("predecessor_cohort_sha256")
        legacy.pop("ledger_sha256"); legacy["ledger_sha256"] = _digest(legacy)
        migrated = build_from_current(source1, current1, legacy, bootstrap=False)
        self.assertEqual(migrated["predecessor_cohort"], [{"raw_tag_id": 315, "name": "Old", "group0": "EXIF", "write_group": "IFD0"}])
        self.assertEqual(build_from_current(source1, current1, migrated, bootstrap=False), migrated)
        tampered = dict(migrated); tampered["predecessor_cohort"] = []; tampered.pop("ledger_sha256"); tampered["ledger_sha256"] = _digest(tampered)
        with self.assertRaisesRegex(RecipeRefused, "cohort"):
            validate_ledger(tampered)
        malformed = dict(migrated); malformed["predecessor_cohort"] = [{**migrated["predecessor_cohort"][0], "name": ""}]; malformed.pop("ledger_sha256"); malformed["ledger_sha256"] = _digest(malformed)
        with self.assertRaisesRegex(RecipeRefused, "malformed"):
            validate_ledger(malformed)

    def test_ambiguous_or_unjoined_final_identity_refuses(self):
        with self.assertRaisesRegex(RecipeRefused, "lacks one exact"):
            self.current([row("Artist", "315"), row("Artist", "0x013b")], [recipe("Artist", 315)])
        with self.assertRaisesRegex(RecipeRefused, "WriteExif"):
            self.current([row("Artist", "315")], [replace(recipe("Artist", 315), write_proc_source_sha256=h("9"))])

    def test_current_physical_or_public_name_collision_refuses(self):
        with self.assertRaisesRegex(RecipeRefused, "duplicate physical"):
            self.current(
                [row("Artist", "315"), row("ArtistAlias", "315")],
                [recipe("Artist", 315), recipe("ArtistAlias", 315)],
            )
        with self.assertRaisesRegex(RecipeRefused, "duplicate public name"):
            self.current(
                [row("Artist", "315"), row("Artist", "316")],
                [recipe("Artist", 315), recipe("Artist", 316)],
            )

    def test_final_writer_source_must_join_address_capture(self):
        with self.assertRaisesRegex(RecipeRefused, "Writer source"):
            self.current([row("Artist", "315")],
                         [replace(recipe("Artist", 315), writer_source_sha256=h("9"))])

    def test_prior_tamper_and_missing_bootstrap_refuse(self):
        source, current = self.current([row("Artist", "315")], [recipe("Artist", 315)])
        with self.assertRaisesRegex(RecipeRefused, "bootstrap"):
            build_from_current(source, current, None, bootstrap=False)
        ledger = build_from_current(source, current, None, bootstrap=True)
        bad = {**ledger, "entries": [{**ledger["entries"][0], "name": "Injected"}]}
        with self.assertRaisesRegex(RecipeRefused, "digest"):
            validate_ledger(bad)

    def test_final_core_source_must_join_address_capture(self):
        with self.assertRaisesRegex(RecipeRefused, "core source"):
            self.current([row("Artist", "315")], [replace(recipe("Artist", 315), main_source_sha256=h("f"))])

    def test_same_source_is_byte_for_byte_noop(self):
        source, current = self.current([row("Artist", "315")], [recipe("Artist", 315)])
        first = build_from_current(source, current, None, bootstrap=True)
        self.assertEqual(build_from_current(source, current, first, bootstrap=False), first)

    def test_compile_current_uses_shared_authenticated_capture_and_final_sources(self):
        synthetic = {"source": "only passed to selected compilers"}
        address = addressing(row("Artist", "315"))
        registry = NativeFormatRegistryArtifact("Image/ExifTool/Exif.pm", h("d"), (), ())
        with patch("setnewvalue_public_migration_ledger.compile_addressing", return_value=(address, {})), \
             patch("setnewvalue_public_migration_ledger._source_capture_identity", return_value=CAPTURE), \
             patch("setnewvalue_public_migration_ledger.compile_final_scalar_stage", return_value=([recipe("Artist", 315)], [], registry)):
            source, current = compile_current(synthetic)
        self.assertEqual(len(current), 1)
        self.assertEqual(source["capture"]["writer_source_sha256"], h("c"))

    def test_renderer_compiles_as_standalone_rust_operand(self):
        source, current = self.current([row("Artist", "315")], [recipe("Artist", 315)])
        rendered = render_rust(build_from_current(source, current, None, bootstrap=True))
        with TemporaryDirectory() as directory:
            source_path = Path(directory) / "public_migration.rs"
            binary = Path(directory) / "public_migration"
            source_path.write_text(rendered + "\nfn main() {}\n", encoding="utf-8")
            subprocess.run(["rustc", "--edition=2021", str(source_path), "-o", str(binary)],
                           check=True, text=True, capture_output=True)


@unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                     "requires explicit canonical EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
class NativePublicMigrationLedgerTest(unittest.TestCase):
    def test_copied_native_name_change_is_new_current_and_old_terminal(self):
        # The source compiler starts from two real selected Exif captures.  No
        # test-side tag-name list supplies either public identity.
        from test_final_scalar_stage import native_document
        lib = Path(os.environ["OXIDEX_PINNED_EXIFTOOL"])
        if (lib / "lib").is_dir():
            lib = lib / "lib"
        first_document = native_document(lib)
        source, current = compile_current(first_document)
        prior = build_from_current(source, current, None, bootstrap=True)
        with TemporaryDirectory() as directory:
            copied = Path(directory) / "lib"
            shutil.copytree(lib, copied)
            exif = copied / "Image/ExifTool/Exif.pm"
            before = exif.read_text(encoding="utf-8")
            needle = "Name => 'HostComputer',\n        Writable => 'string',"
            self.assertIn(needle, before)
            exif.write_text(before.replace(needle, "Name => 'RenamedHost',\n        Writable => 'string',", 1),
                            encoding="utf-8")
            changed_document = native_document(copied)
        changed_source, changed_current = compile_current(changed_document)
        upgraded = build_from_current(changed_source, changed_current, prior, bootstrap=False)
        states = {entry["name"]: entry["state"] for entry in upgraded["entries"]}
        self.assertEqual(states["HostComputer"], "removed_or_unsupported")
        self.assertEqual(states["RenamedHost"], "current")


if __name__ == "__main__":
    unittest.main()
