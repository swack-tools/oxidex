"""Model-Condition controls for gen_sony_main_extra_tables.py.

A `Sony::Main` Condition decides which cameras get a tag, so each release's
text must translate to that release's own model set -- never collapsed onto
the pinned release's. The Condition strings below are copied by hand out of
each release's Sony.pm (`0x201d` and the first `0x2020` variant), independent
of both the generator's dictionary and the fixture.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

import gen_sony_main_extra_tables as generator

HERE = Path(__file__).resolve().parent
FIXTURE = HERE / "testdata" / "sony_main_model_conditions.json"
PIN = (HERE.parents[1] / ".exiftool-version").read_text().strip()

FLEX = {
    "11.78": ("$$self{Model} =~ /^(NEX-|ILCE-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX99|RX0M2))/",
              'MCond::ModelRe(false, r"^(NEX-|ILCE-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX99|RX0M2))")'),
    "12.64": ("$$self{Model} =~ /^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2))/",
              'MCond::ModelRe(false, r"^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2))")'),
    "13.59": ("$$self{Model} =~ /^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2|RX1RM3))/",
              'MCond::ModelRe(false, r"^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2|RX1RM3))")'),
}
AF_USED = {
    "11.78": ("$$self{Model} !~ /^(ILCA-|DSC-)/", 'MCond::ModelRe(true, r"^(ILCA-|DSC-)")'),
    "12.64": ("$$self{Model} !~ /^(ILCA-|DSC-)/", 'MCond::ModelRe(true, r"^(ILCA-|DSC-)")'),
    "13.59": ("$$self{Model} !~ /^(ILCA-|DSC-|ZV-)/", 'MCond::ModelRe(true, r"^(ILCA-|DSC-|ZV-)")'),
}
MODEL_COND = re.compile(r"^\$\$self\{Model\} (=~|!~) /(.*)/$")
MODEL_RE = re.compile(r'^MCond::ModelRe\((true|false), r"(.*)"\)$')


class ReleaseConditions(unittest.TestCase):
    def test_each_releases_text_translates_to_its_own_regex(self):
        for table, tag in ((FLEX, 0x201D), (AF_USED, 0x2020)):
            for release, (perl, rust) in table.items():
                with self.subTest(release=release, tag=hex(tag)):
                    self.assertEqual(generator.translate_cond(tag, "t", perl), rust)

    def test_releases_are_not_collapsed(self):
        # Three distinct FlexibleSpotPosition texts, three distinct regexes.
        self.assertEqual(len({generator.translate_cond(0x201D, "t", p) for p, _ in FLEX.values()}), 3)

    def test_pinned_entries_are_unchanged(self):
        self.assertEqual(generator.COND_DICT[FLEX["13.59"][0]], FLEX["13.59"][1])
        self.assertEqual(generator.COND_DICT[AF_USED["13.59"][0]], AF_USED["13.59"][1])

    def test_every_model_regex_is_copied_verbatim(self):
        # Any `$$self{Model} =~/!~ /BODY/` key must emit exactly BODY with the
        # right negation: the translation is a copy, not a rewrite.
        seen = 0
        for key, value in generator.COND_DICT.items():
            m = MODEL_COND.match(key)
            if not m:
                continue
            seen += 1
            r = MODEL_RE.match(value)
            self.assertIsNotNone(r, key)
            self.assertEqual(r.group(1), "true" if m.group(1) == "!~" else "false", key)
            self.assertEqual(r.group(2), m.group(2), key)
        self.assertGreaterEqual(seen, 10)

    def test_historical_bodies_use_only_the_portable_subset(self):
        # Anchors, literals, `-`, `|` and groups mean the same thing to Perl
        # and to the `regex` crate; anything else would need its own argument.
        for table in (FLEX, AF_USED):
            for release, (perl, _) in table.items():
                body = MODEL_COND.match(perl).group(2)
                self.assertRegex(body, r"^\^[A-Za-z0-9|()\-]+$", release)

    def test_an_unregistered_model_condition_still_refuses(self):
        with self.assertRaises(generator.Unsupported):
            generator.translate_cond(0x201D, "FlexibleSpotPosition",
                                     "$$self{Model} =~ /^(NEX-|ILCE-|DSC-RX10M4)/")


class Fixture(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = json.loads(FIXTURE.read_text())

    def rows(self, version, tag):
        rel = [r for r in self.data["releases"] if r["exiftool_version"] == version]
        self.assertEqual(len(rel), 1, version)
        return [c for c in rel[0]["conditions"] if c["tag"] == tag]

    def test_fixture_covers_the_three_releases(self):
        self.assertEqual(sorted(r["exiftool_version"] for r in self.data["releases"]),
                         ["11.78", "12.64", PIN])

    def test_fixture_texts_are_the_releases_texts(self):
        for table, tag in ((FLEX, "0x201d"), (AF_USED, "0x2020")):
            for release, (perl, rust) in table.items():
                row = self.rows(release, tag)[0]
                self.assertEqual(row["condition"], perl)
                self.assertEqual(row["generated"], rust)

    def test_fixture_generated_text_is_current(self):
        for rel in self.data["releases"]:
            for row in rel["conditions"]:
                self.assertEqual(
                    generator.translate_cond(int(row["tag"], 16), row["name"], row["condition"]),
                    row["generated"], (rel["exiftool_version"], row["tag"]))

    def test_the_releases_select_different_cameras(self):
        # The point of not collapsing: 12.64 adds ILME-/ZV-/DSC-HX95 and 13.59
        # adds DSC-RX1RM3 (unanchored at the end, so the synthetic DSC-RX1RM3X
        # too) -- each a set difference the releases' own Perl produced.
        sel = {v: set(self.rows(v, "0x201d")[0]["selects"]) for v in FLEX}
        self.assertTrue({"ILME-FX3", "ZV-E10", "DSC-HX95"} <= sel["12.64"] - sel["11.78"])
        self.assertEqual(sel["13.59"] - sel["12.64"], {"DSC-RX1RM3", "DSC-RX1RM3X"})
        self.assertEqual(sel["11.78"] - sel["12.64"], set())
        af = {v: set(self.rows(v, "0x2020")[0]["selects"]) for v in AF_USED}
        self.assertTrue({"ZV-1", "ZV-E10"} <= af["12.64"] - af["13.59"])

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                         "needs EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
    def test_pinned_release_perl_still_selects_the_fixture(self):
        lib = Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]) / "lib"
        models = self.data["models"]["real"] + self.data["models"]["synthetic"]
        with tempfile.NamedTemporaryFile("w", suffix=".json") as tmp:
            json.dump(models, tmp)
            tmp.flush()
            out = subprocess.run(
                [os.environ["EXIFTOOL_PERL"], str(HERE / "capture_sony_main_conditions.pl"), "eval",
                 str(lib), tmp.name, *(f"0x{i:x}" for i in generator.MANIFEST)],
                capture_output=True, text=True, check=True)
        live = json.loads(out.stdout)
        self.assertEqual(live["exiftool_version"], PIN)
        fixture = [r for r in self.data["releases"] if r["exiftool_version"] == PIN][0]
        strip = lambda rows: [{k: v for k, v in r.items() if k != "generated"} for r in rows]
        self.assertEqual(strip(live["conditions"]), strip(fixture["conditions"]))
        self.assertEqual(live["sony_pm_sha256"], fixture["sony_pm_sha256"])


class Header(unittest.TestCase):
    def test_header_names_the_dumped_release(self):
        data = {"exiftool_version": "11.78", "modules": {"Sony": {"tables": {"Main": {"tags": {}}}}}}
        saved = generator.MANIFEST[:]
        try:
            generator.MANIFEST[:] = []
            text = generator.generate(data)
        finally:
            generator.MANIFEST[:] = saved
        self.assertIn("in-process\n//! (11.78) rather", text)
        data["exiftool_version"] = "11.78; rm"
        generator.MANIFEST[:] = []
        try:
            with self.assertRaises(generator.Unsupported):
                generator.generate(data)
        finally:
            generator.MANIFEST[:] = saved



PERL = os.environ.get("EXIFTOOL_PERL") or shutil.which("perl")
EVIDENCE_DUMPS = Path(os.environ.get("OXIDEX_TEST_REHEARSAL_DUMPS",
                                     "/Users/allen/oxidex-ops/evidence/20260917-rehearsal-diag/dumps"))
from test_module_entry_absence import LIB_1178, LIB_1264, make_lib  # noqa: E402


def dump(tags, version="11.78"):
    return {"exiftool_version": version, "modules": {"Sony": {"tables": {"Main": {"tags": tags}}}}}


@unittest.skipUnless(PERL, "needs a perl")
class ProvenAbsentIds(unittest.TestCase):
    """A MANIFEST id missing from the dump emits no row only when proven absent."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        saved = generator.MANIFEST[:]
        self.addCleanup(generator.MANIFEST.__setitem__, slice(None), saved)
        generator.MANIFEST[:] = [0x2031, 0x2032, 0xB041]
        # The fake lib's Sony::Main holds 0x2031 and 0xb041, not 0x2032.
        self.data = dump({"8241": {"Name": "SerialNumber"}, "45121": {"Name": "ExposureMode"}})

    def rows(self, text):
        return re.findall(r"^    MainExtraTag \{ id: (0x[0-9a-f]+), name: \"([^\"]+)\"", text, re.M)

    def test_proven_absent_id_emits_no_row_and_names_its_proof(self):
        lib = make_lib(self.tmp)
        text = generator.generate(self.data, lib, PERL)
        self.assertEqual(self.rows(text), [("0x2031", "SerialNumber"), ("0xb041", "ExposureMode")])
        self.assertIn("// Absent: `0x2032`.", text)
        self.assertIn("prove_entry_absent", text)
        self.assertIn("loaded Sony::Main: 2 tag ids", text)

    def test_missing_id_without_a_lib_still_refuses(self):
        with self.assertRaisesRegex(generator.Unsupported, "0x2032.*--exiftool-lib"):
            generator.generate(self.data)

    def test_missing_id_the_release_names_refuses(self):
        # The dump lacks 0x2032 but the release's Sony.pm defines it (changed or
        # not): the dump is not this release's table, and nothing is invented.
        lib = make_lib(self.tmp, extra="    0x2032 => { Name => 'Changed' },\n")
        with self.assertRaisesRegex(generator.Unsupported, "not proven absent"):
            generator.generate(self.data, lib, PERL)

    def test_dump_from_another_table_refuses(self):
        lib = make_lib(self.tmp)
        data = dump({"8241": {"Name": "SerialNumber"}, "45121": {"Name": "ExposureMode"},
                     "45122": {"Name": "Extra"}})
        with self.assertRaisesRegex(generator.Unsupported, "differ from the table loaded"):
            generator.generate(data, lib, PERL)

    def test_present_id_with_an_unregistered_definition_still_refuses(self):
        lib = make_lib(self.tmp)
        self.data["modules"]["Sony"]["tables"]["Main"]["tags"]["8241"]["Condition"] = "$$self{Model} eq 'X'"
        with self.assertRaisesRegex(generator.Unsupported, "0x2031.*unregistered Condition"):
            generator.generate(self.data, lib, PERL)

    def test_all_present_emits_no_absence_note(self):
        generator.MANIFEST[:] = [0x2031, 0xB041]
        text = generator.generate(self.data, make_lib(self.tmp), PERL)
        self.assertNotIn("Absent:", text)
        self.assertEqual(text, generator.generate(self.data))


@unittest.skipUnless(PERL and LIB_1178.is_dir() and LIB_1264.is_dir()
                     and (EVIDENCE_DUMPS / "tables-11.78.json").is_file()
                     and (EVIDENCE_DUMPS / "tables-12.64.json").is_file(),
                     "needs the 11.78/12.64 source trees and their dumps")
class HistoricalGeneration(unittest.TestCase):
    def generate(self, release, lib):
        data = json.loads((EVIDENCE_DUMPS / f"tables-{release}.json").read_text())
        return generator.generate(data, lib, PERL)

    def test_1178(self):
        text = self.generate("11.78", LIB_1178)
        self.assertEqual(len(re.findall(r"^    MainExtraTag \{", text, re.M)), 26)
        self.assertIn("// Absent: `0x2032`, `0x2033`, `0x2034`, `0x2035`, `0x2036`, `0x2037`, "
                      "`0x2039`, `0x204a`, `0x205c`.", text)

    def test_1264(self):
        text = self.generate("12.64", LIB_1264)
        self.assertEqual(len(re.findall(r"^    MainExtraTag \{", text, re.M)), 33)
        self.assertIn("// Absent: `0x204a`, `0x205c`.", text)

    def test_a_dump_against_the_wrong_release_refuses(self):
        data = json.loads((EVIDENCE_DUMPS / "tables-12.64.json").read_text())
        with self.assertRaisesRegex(generator.Unsupported, "differ from the table loaded"):
            generator.generate(data, LIB_1178, PERL)


if __name__ == "__main__":
    unittest.main()
