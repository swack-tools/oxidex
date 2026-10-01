"""XMP tag priorities: committed artifacts replay from the pinned capture."""
from __future__ import annotations

import copy
import json
import os
import subprocess
import unittest
from pathlib import Path

import xmp_priority_specs as specs

HERE = Path(__file__).resolve().parent
FRESH_INPUTS = ("EXIFTOOL_PERL", "OXIDEX_PINNED_EXIFTOOL")


class XmpPrioritySpecTests(unittest.TestCase):
    def setUp(self):
        self.capture = json.loads(specs.FIXTURE.read_text())

    def test_committed_rust_replays_from_the_fixture(self):
        from join_catalog_hydrated import quicktime_rust_matches
        self.assertTrue(quicktime_rust_matches(specs.render(self.capture), specs.RUST.read_text()))

    def test_capture_is_internally_consistent(self):
        self.assertEqual(self.capture["exiftool_version"], (specs.ROOT / ".exiftool-version").read_text().strip())
        namespaces = self.capture["namespaces"]
        if specs.PIN != "13.59":
            self.assertTrue(namespaces)
            specs.validate(self.capture)
            return
        # FoundXMP looks tables up by the prefix after %stdXlatNS.
        self.assertIn("iptcCore", namespaces)
        self.assertNotIn("Iptc4xmpCore", namespaces)
        self.assertEqual(namespaces["iptcCore"]["namespace"], "Iptc4xmpCore")
        self.assertEqual(namespaces["photomech"]["table"], "Image::ExifTool::PhotoMechanic::XMP")
        self.assertEqual(namespaces["tiff"]["table_priority"], 0)
        self.assertTrue(all(tag["priority"] == 0 for tag in namespaces["exif"]["tags"].values()))
        self.assertEqual(namespaces["pdf"]["tags"]["Keywords"]["own_priority"], -1)
        self.assertEqual(namespaces["xmp"]["tags"]["CreateDate"]["priority"], 0)
        self.assertTrue(namespaces["dc"]["tags"]["title"]["lang_alt"])
        self.assertFalse(namespaces["dc"]["tags"]["subject"]["lang_alt"])
        # Keyed by raw tag ID: cc:legalcode is named LegalCode.
        self.assertEqual(namespaces["cc"]["tags"]["legalcode"]["name"], "LegalCode")
        self.assertNotIn("LegalCode", namespaces["cc"]["tags"])
        # Flattened structure IDs are entries too, and variable-namespace
        # structures are marked.
        self.assertIn("FlashFired", namespaces["exif"]["tags"])
        self.assertEqual(namespaces["iptcExt"]["tags"]["ImageRegion"]["struct"], "variable")
        self.assertEqual(self.capture["xmp_ns"]["iptcExt"], "Iptc4xmpExt")
        special = self.capture["special_tables"]
        self.assertEqual(special["SVG"]["group0"], "SVG")
        self.assertEqual(special["SVG"]["tags"]["width"]["name"], "ImageWidth")
        self.assertEqual(special["otherSVG"]["tags"]["c2pa:manifest"]["name"], "JUMBF")
        self.assertIn("dc", special["XML"]["tags"])
        self.assertIn("lastUpdate", special["XML"]["tags"])
        for mutate in (lambda c: c.update(extra=1),
                       lambda c: c["namespaces"]["dc"].update(extra=1),
                       lambda c: c["namespaces"]["dc"]["tags"]["title"].update(lang_alt="true"),
                       lambda c: c["namespaces"]["dc"]["tags"]["title"].update(lang_alt=None),
                       lambda c: c["namespaces"]["dc"]["tags"]["title"].update(priority="1"),
                       lambda c: c["namespaces"]["dc"]["tags"]["title"].update(priority=True),
                       lambda c: c["namespaces"]["dc"]["tags"]["title"].update(struct="odd"),
                       lambda c: c["namespaces"]["dc"]["tags"]["title"].update(avoid=1),
                       lambda c: c["namespaces"]["dc"]["tags"].update({'Q"uote': c["namespaces"]["dc"]["tags"]["title"]}),
                       lambda c: c["namespaces"]["dc"].update(table_priority="0"),
                       lambda c: c.update(xmp_ns={}),
                       lambda c: c["special_tables"].pop("SVG"),
                       lambda c: c["special_tables"]["XML"].update(group0=42)):
            broken = copy.deepcopy(self.capture); mutate(broken)
            with self.assertRaises(ValueError):
                specs.validate(broken)

    def test_generated_names_preserve_source_names_distinct_from_raw_ids(self):
        rendered = specs.render(self.capture)
        for section in ("namespaces", "special_tables"):
            for table in self.capture[section].values():
                for raw_id, fact in table["tags"].items():
                    self.assertIn(
                        f'XmpTagFact {{ id: "{raw_id}", name: "{fact["name"]}",',
                        rendered,
                    )
        if specs.PIN == "13.59":
            owner = self.capture["namespaces"]["plus"]["tags"]["CopyrightOwnerCopyrightOwnerName"]
            self.assertEqual(owner["name"], "CopyrightOwnerName")

    def test_generated_names_preserve_source_names_distinct_from_raw_ids(self):
        rendered = specs.render(self.capture)
        for section in ("namespaces", "special_tables"):
            for table in self.capture[section].values():
                for raw_id, fact in table["tags"].items():
                    self.assertIn(
                        f'XmpTagFact {{ id: "{raw_id}", name: "{fact["name"]}",',
                        rendered,
                    )
        if specs.PIN == "13.59":
            owner = self.capture["namespaces"]["plus"]["tags"]["CopyrightOwnerCopyrightOwnerName"]
            self.assertEqual(owner["name"], "CopyrightOwnerName")

    def test_fixture_is_the_fresh_pinned_capture(self):
        missing = [name for name in FRESH_INPUTS if not os.environ.get(name)]
        if missing:
            if os.environ.get("GITHUB_ACTIONS"):
                self.fail(f"fresh pinned-source inputs missing in CI: {missing}")
            self.skipTest(f"set {', '.join(FRESH_INPUTS)} for the pinned source")
        source = Path(os.environ["OXIDEX_PINNED_EXIFTOOL"])
        library = source / "lib" if (source / "lib").is_dir() else source
        env = {key: value for key, value in os.environ.items() if not key.startswith("PERL5")}
        run = subprocess.run([os.environ["EXIFTOOL_PERL"], str(HERE / "capture_xmp_priorities.pl"), str(library)],
                             capture_output=True, text=True, env=env)
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(json.loads(run.stdout), self.capture)


if __name__ == "__main__":
    unittest.main()
