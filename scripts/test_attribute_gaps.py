"""Hermetic tests for attribute_gaps.py (spec S1/S2).

Everything runs against a synthetic Perl lib and synthetic comparison
JSON built in tempdirs -- no dependency on the real ExifTool install,
the real corpus, or ~/.oxidex. The one repo file exercised is the
checked-in config.example.toml's [squads.*] tables, asserted against the
spec S2 squad table they must encode (config.example.toml, not config.toml,
because config.toml is gitignored/per-installation and may not exist in a
fresh checkout -- see CheckedInSquadsTomlTests).
"""
import json
import tempfile
import unittest
from pathlib import Path

from attribute_gaps import (
    FALLBACK_SQUAD,
    UNKNOWN_MODULE,
    attribute_gap,
    build_attribution,
    build_tag_index,
    extract_sample_dir,
    format_summary,
    load_squads,
    main,
    write_atomic,
)

SCRIPTS_DIR = Path(__file__).resolve().parent

CANON_PM = """\
# comment noise, incl. an unbalanced ( paren and a 'Name => quote
%Image::ExifTool::Canon::Main = (
    GROUPS => { 0 => 'MakerNotes', 2 => 'Camera' },
    0x1 => {
        Name => 'CanonCameraSettings',
        SubDirectory => { TagTable => 'Image::ExifTool::Canon::CameraSettings' },
    },
    0x6 => 'CanonImageType',
    LensStats => {
        Name => 'LensStats',
    },
);

%Image::ExifTool::Canon::CameraSettings = (
    PROCESS_PROC => \\&Image::ExifTool::ProcessBinaryData,
    1 => {
        Name => 'MacroMode',
        PrintConv => {
            1 => 'Macro',
            2 => 'Normal',
        },
    },
    2 => 'SelfTimer',
);
"""

NIKON_PM = """\
%Image::ExifTool::Nikon::Main = (
    0x2 => { Name => 'ISOSetting' },
    0x4 => 'Quality',
    0x8 => { Name => 'MacroMode' }, # collides with Canon's MacroMode
);
"""

# XMP2.pl-style: filename stem ("Extra") differs from the declared
# package module ("XMP"); bare lowercase property keys.
EXTRA_PL = """\
%Image::ExifTool::XMP::crd = (
    GROUPS => { 2 => 'Image' },
    someProperty => { },
    Version => { Writable => 'string' },
);
"""

CONFIG_TOML = """\
[meta]
snapshot_date = "2026-07-24"

[squads.canon]
modules = ["Canon"]
formats = ["JPEG", "CR2"]
ownership_globs = []

[squads.nikon]
modules = ["Nikon"]
formats = ["JPEG", "NEF"]
ownership_globs = []

[squads.tail]
modules = []
formats = []
ownership_globs = []
"""


def make_perl_lib(tmpdir):
    """Write the two mini .pm files (+ one .pl) and return the dir."""
    lib = Path(tmpdir) / "perl-lib"
    lib.mkdir()
    (lib / "Canon.pm").write_text(CANON_PM)
    (lib / "Nikon.pm").write_text(NIKON_PM)
    (lib / "Extra.pl").write_text(EXTRA_PL)
    return lib


def make_config_toml(tmpdir, content=CONFIG_TOML):
    path = Path(tmpdir) / "config.toml"
    path.write_text(content)
    return path


def sample(name, family, source_file):
    return {"name": name, "family": family, "value": "x", "tag_id": None,
            "source_file": source_file}


def make_report():
    """Synthetic ComparisonReport covering both gap kinds and both
    disambiguation paths (format priority for NEF, sample-dir hint for
    JPEG)."""
    return {
        "by_format": {
            "JPEG": {
                "missing_in_oxidex": [
                    sample("MacroMode", "MakerNotes",
                           "/c/combined-samples/Canon/a.jpg"),
                    sample("NoSuchTagAnywhere", "MakerNotes",
                           "/c/combined-samples/b.jpg"),
                    sample("CanonImageType", "MakerNotes",
                           "/c/combined-samples/Canon/a.jpg"),
                ],
                "value_differences": [
                    {"tag_key": "MakerNotes:Quality",
                     "exiftool_value": "Fine", "oxidex_value": "0",
                     "source_file": "/c/combined-samples/Nikon/n.jpg"},
                ],
            },
            "NEF": {
                "missing_in_oxidex": [
                    sample("MacroMode", "MakerNotes",
                           "/c/combined-samples/Nikon/n.nef"),
                ],
                "value_differences": [],
            },
        }
    }


class TagIndexTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.index, self.modules = build_tag_index(make_perl_lib(self._tmp.name))

    def test_name_arrow_entries_indexed_with_module_and_table(self):
        self.assertIn(("Canon", "Main"), self.index["CanonCameraSettings"])
        self.assertIn(("Canon", "CameraSettings"), self.index["MacroMode"])
        self.assertIn(("Nikon", "Main"), self.index["MacroMode"])

    def test_simple_id_shorthand_indexed_at_table_top_level(self):
        self.assertEqual(self.index["CanonImageType"], [("Canon", "Main")])
        self.assertEqual(self.index["SelfTimer"], [("Canon", "CameraSettings")])
        self.assertEqual(self.index["Quality"], [("Nikon", "Main")])

    def test_printconv_values_not_indexed_as_tags(self):
        # 1 => 'Macro' / 2 => 'Normal' sit inside a PrintConv hash
        # (depth 2) and must not become tag names.
        self.assertNotIn("Normal", self.index)
        self.assertNotIn("Macro", self.index)

    def test_bare_key_entries_indexed_but_not_allcaps_metadata(self):
        self.assertIn(("Canon", "Main"), self.index["LensStats"])
        self.assertNotIn("GROUPS", self.index)
        self.assertNotIn("PROCESS_PROC", self.index)

    def test_declared_package_module_wins_over_filename(self):
        # Extra.pl declares %Image::ExifTool::XMP::crd -- module must
        # index as XMP (the XMP2.pl case), not "Extra".
        self.assertEqual(self.index["Version"], [("XMP", "crd")])
        self.assertNotIn(("Extra", "crd"), self.index["Version"])
        self.assertIn("XMP", self.modules)

    def test_lowercase_bare_keys_also_indexed_ucfirst(self):
        self.assertIn(("XMP", "crd"), self.index["someProperty"])
        self.assertIn(("XMP", "crd"), self.index["SomeProperty"])


class AttributeGapTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.index, modules = build_tag_index(make_perl_lib(self._tmp.name))
        self.lookup = {m.lower(): m for m in modules}

    def test_format_priority_disambiguates_collision(self):
        # MacroMode exists in Canon.pm AND Nikon.pm; NEF's priority
        # list puts Nikon first.
        module, table = attribute_gap(
            "NEF", "MakerNotes", "MacroMode", self.index, self.lookup)
        self.assertEqual((module, table), ("Nikon", "Main"))

    def test_sample_dir_hint_disambiguates_when_no_format_priority(self):
        module, table = attribute_gap(
            "JPEG", "MakerNotes", "MacroMode", self.index, self.lookup,
            sample_dirs=["Canon"])
        self.assertEqual((module, table), ("Canon", "CameraSettings"))
        module, _ = attribute_gap(
            "JPEG", "MakerNotes", "MacroMode", self.index, self.lookup,
            sample_dirs=["Nikon"])
        self.assertEqual(module, "Nikon")

    def test_sample_dir_alias_leica_maps_to_panasonic(self):
        index = {"WhiteBalanceBias": [("Panasonic", "Main"), ("Sony", "Main")]}
        module, _ = attribute_gap(
            "JPEG", "MakerNotes", "WhiteBalanceBias", index, {},
            sample_dirs=["Leica"])
        self.assertEqual(module, "Panasonic")

    def test_family_data_driven_match_is_case_insensitive(self):
        module, table = attribute_gap(
            "JPEG", "CANON", "CanonImageType", self.index, self.lookup)
        self.assertEqual((module, table), ("Canon", "Main"))

    def test_family_override_beats_index_candidates(self):
        # Family EXIF routes to Exif even though the name only exists
        # in Canon.pm here; the index then has no Exif table for it.
        module, table = attribute_gap(
            "JPEG", "EXIF", "CanonImageType", self.index, self.lookup)
        self.assertEqual((module, table), ("Exif", ""))

    def test_makernotes_family_never_maps_to_makernotes_module(self):
        # Even with a MakerNotes.pm-style module present, the family
        # override forces the index path.
        lookup = dict(self.lookup, makernotes="MakerNotes")
        module, _ = attribute_gap(
            "NEF", "MakerNotes", "MacroMode", self.index, lookup)
        self.assertEqual(module, "Nikon")

    def test_unknown_fallback_for_unindexed_names(self):
        module, table = attribute_gap(
            "JPEG", "MakerNotes", "NoSuchTagAnywhere", self.index, self.lookup)
        self.assertEqual((module, table), (UNKNOWN_MODULE, ""))

    def test_deterministic_fallback_without_priority_or_hint(self):
        module, table = attribute_gap(
            "JPEG", "MakerNotes", "MacroMode", self.index, self.lookup)
        self.assertEqual((module, table), ("Canon", "CameraSettings"))


class ExtractSampleDirTests(unittest.TestCase):
    def test_subdir_sample(self):
        self.assertEqual(
            extract_sample_dir("/x/combined-samples/Nikon/a.jpg"), "Nikon")

    def test_root_sample_has_no_dir(self):
        self.assertIsNone(extract_sample_dir("/x/combined-samples/a.cr3"))

    def test_unrecognised_path_and_none(self):
        self.assertIsNone(extract_sample_dir("/somewhere/else/a.jpg"))
        self.assertIsNone(extract_sample_dir(None))


class BuildAttributionTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.index, self.modules = build_tag_index(
            make_perl_lib(self._tmp.name))
        self.module_to_squad, self.squad_names = load_squads(
            make_config_toml(self._tmp.name))

    def build(self, formats=None):
        return build_attribution(
            make_report(), self.index, self.modules, self.module_to_squad,
            self.squad_names, formats=formats, now_iso="2026-07-24T00:00:00+00:00")

    def test_tag_records_have_spec_shape(self):
        out = self.build()
        rec = out["tags"]["JPEG:MakerNotes:MacroMode"]
        self.assertEqual(rec, {
            "module": "Canon", "table": "CameraSettings", "squad": "canon",
            "formats": ["JPEG"], "sample_dirs": ["Canon"],
        })
        self.assertEqual(out["generated_at"], "2026-07-24T00:00:00+00:00")

    def test_same_tag_attributes_per_format(self):
        out = self.build()
        self.assertEqual(out["tags"]["NEF:MakerNotes:MacroMode"]["module"],
                         "Nikon")
        self.assertEqual(out["tags"]["NEF:MakerNotes:MacroMode"]["squad"],
                         "nikon")

    def test_value_differences_count_as_gaps(self):
        out = self.build()
        rec = out["tags"]["JPEG:MakerNotes:Quality"]
        self.assertEqual((rec["module"], rec["squad"]), ("Nikon", "nikon"))

    def test_unknown_module_rolls_up_to_tail(self):
        out = self.build()
        rec = out["tags"]["JPEG:MakerNotes:NoSuchTagAnywhere"]
        self.assertEqual(rec["module"], UNKNOWN_MODULE)
        self.assertEqual(rec["squad"], FALLBACK_SQUAD)

    def test_squad_rollup_counts_formats_modules(self):
        out = self.build()
        self.assertEqual(out["squads"]["canon"], {
            "open_gaps": 2, "formats": ["JPEG"], "modules": ["Canon"]})
        self.assertEqual(out["squads"]["nikon"], {
            "open_gaps": 2, "formats": ["JPEG", "NEF"], "modules": ["Nikon"]})
        self.assertEqual(out["squads"]["tail"]["open_gaps"], 1)
        self.assertIn(UNKNOWN_MODULE, out["squads"]["tail"]["modules"])

    def test_all_configured_squads_present_even_with_zero_gaps(self):
        out = self.build(formats={"NEF"})
        self.assertEqual(out["squads"]["canon"]["open_gaps"], 0)
        self.assertEqual(out["squads"]["tail"]["open_gaps"], 0)

    def test_formats_filter(self):
        out = self.build(formats={"NEF"})
        self.assertEqual(list(out["tags"]), ["NEF:MakerNotes:MacroMode"])

    def test_summary_mentions_squads_and_unknown_count(self):
        text = format_summary(self.build())
        self.assertIn("canon", text)
        self.assertIn("1 unattributable", text)


class WriteAtomicTests(unittest.TestCase):
    def test_writes_json_and_leaves_no_tempfiles(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            out = Path(tmpdir) / "nested" / "gap-attribution.json"
            write_atomic(out, {"a": 1})
            self.assertEqual(json.loads(out.read_text()), {"a": 1})
            leftovers = [p for p in out.parent.iterdir() if p != out]
            self.assertEqual(leftovers, [])

    def test_replaces_existing_content_atomically(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            out = Path(tmpdir) / "out.json"
            write_atomic(out, {"v": 1})
            write_atomic(out, {"v": 2})
            self.assertEqual(json.loads(out.read_text()), {"v": 2})

    def test_failed_write_keeps_original_and_cleans_tmp(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            out = Path(tmpdir) / "out.json"
            write_atomic(out, {"v": 1})
            with self.assertRaises(TypeError):
                write_atomic(out, {"v": {1, 2}})  # sets aren't JSON
            self.assertEqual(json.loads(out.read_text()), {"v": 1})
            leftovers = [p for p in Path(tmpdir).iterdir() if p != out]
            self.assertEqual(leftovers, [])


class MainCliTests(unittest.TestCase):
    def run_main(self, extra_args=()):
        with tempfile.TemporaryDirectory() as tmpdir:
            lib = make_perl_lib(tmpdir)
            squads = make_config_toml(tmpdir)
            comparison = Path(tmpdir) / "comparison.json"
            comparison.write_text(json.dumps(make_report()))
            out = Path(tmpdir) / "gap-attribution.json"
            rc = main([
                "--comparison", str(comparison),
                "--perl-lib", str(lib),
                "--config", str(squads),
                "--out", str(out),
                *extra_args,
            ])
            self.assertEqual(rc, 0)
            return json.loads(out.read_text())

    def test_end_to_end(self):
        out = self.run_main()
        self.assertEqual(len(out["tags"]), 5)
        self.assertEqual(out["squads"]["canon"]["open_gaps"], 2)

    def test_formats_filter_and_summary_flag(self):
        out = self.run_main(["--formats", "NEF", "--print-summary"])
        self.assertEqual(list(out["tags"]), ["NEF:MakerNotes:MacroMode"])

    def test_missing_perl_lib_errors(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            comparison = Path(tmpdir) / "c.json"
            comparison.write_text("{}")
            rc = main(["--comparison", str(comparison),
                       "--perl-lib", str(Path(tmpdir) / "nope"),
                       "--config", str(make_config_toml(tmpdir)),
                       "--out", str(Path(tmpdir) / "o.json")])
            self.assertEqual(rc, 1)


class CheckedInSquadsTomlTests(unittest.TestCase):
    """Guard the checked-in squad manifest against the spec S2 table.

    Reads config.example.toml, not config.toml: config.toml is gitignored
    and per-installation (it may not exist at all in a fresh checkout or
    CI), while config.example.toml is the git-tracked template that carries
    the real, current [squads.*] tables verbatim (see the PR that moved
    scripts/squads.toml's content into config.toml) -- exactly the "checked
    -in" file this test class name promises to guard.
    """

    SPEC_SQUADS = {
        "canon": ["Canon", "CanonCustom", "CanonRaw", "CanonVRD", "QuickTime"],
        "nikon": ["Nikon", "NikonCustom", "NikonSettings", "NikonCapture"],
        "sony-minolta": ["Sony", "Minolta", "MinoltaRaw"],
        "xmp": ["XMP"],
        "exif-core": ["Exif"],
        "olympus": ["Olympus"],
        "pentax-samsung": ["Pentax", "Samsung"],
        "panasonic-leica": ["Panasonic", "PanasonicRaw"],
        "mobile": ["Google", "GoPro", "Apple", "DJI", "Qualcomm"],
        "thermal": ["FLIR", "InfiRay"],
        "sigma-c2pa": ["Sigma", "SigmaRaw", "Jpeg2000"],
        "ps-docs": ["Photoshop", "IPTC", "PhotoMechanic", "FotoStation", "PDF"],
        "standards-appn": ["ICC_Profile", "JPEG", "APP12", "Meta", "MPF"],
        "tail": ["FlashPix", "Kodak", "Sanyo", "Ricoh", "Casio", "FujiFilm"],
    }

    def setUp(self):
        import tomllib
        self.manifest_path = SCRIPTS_DIR.parent / "config.example.toml"
        with open(self.manifest_path, "rb") as f:
            self.data = tomllib.load(f)

    def test_fourteen_squads_with_spec_modules(self):
        squads = self.data["squads"]
        self.assertEqual(sorted(squads), sorted(self.SPEC_SQUADS))
        for name, modules in self.SPEC_SQUADS.items():
            self.assertEqual(squads[name]["modules"], modules, name)

    def test_every_squad_has_empty_ownership_globs_for_now(self):
        for name, cfg in self.data["squads"].items():
            self.assertEqual(cfg["ownership_globs"], [], name)
            self.assertIn("formats", cfg, name)

    def test_no_stored_gap_counts(self):
        # Gap counts are deliberately NOT stored in the squad manifest --
        # they are derived live, every round, by attribute_gaps.py itself
        # (see the "squads" summary in gap-attribution.json). [meta] /
        # snapshot_date used to record the census date this table was taken
        # from; it was dropped when the manifest moved into config.toml
        # (comment-equivalent, consumed by no code) -- see the manifest's
        # own header comment in config.example.toml for the same date.
        for name, cfg in self.data["squads"].items():
            self.assertNotIn("gaps", cfg, name)
            self.assertNotIn("open_gaps", cfg, name)

    def test_loader_maps_modules_and_defaults_to_tail(self):
        module_to_squad, squad_names = load_squads(self.manifest_path)
        self.assertEqual(len(squad_names), 14)
        self.assertEqual(module_to_squad["CanonCustom"], "canon")
        self.assertEqual(module_to_squad["Jpeg2000"], "sigma-c2pa")
        self.assertEqual(module_to_squad.get("Nonexistent", FALLBACK_SQUAD),
                         FALLBACK_SQUAD)


if __name__ == "__main__":
    unittest.main()
