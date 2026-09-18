"""A release without `Image::ExifTool::Garmin` is an explicit, recorded state.

ExifTool first shipped Garmin.pm (and FIT reading) in 13.56; 11.78 and 12.64
route the `.FIT` extension to FITS and define no FIT tag. For such a release
`capture_garmin_fit_fact.pl` records a `garmin_fit_module_absent_v1` fact,
proven from the selected source tree, and the generator emits zero FIT rows
and a Rust protocol that extracts nothing. These tests pin that:

* the absent fact is recorded as absent, and only when it proves absence for
  this dump's own release;
* a genuine capture failure (a crash, an empty or unparsable fact, a tree that
  references Garmin without the module file, a module that fails to load)
  is still refused, never mistaken for absence;
* the pinned 13.59 artifacts are unchanged (see also test_garmin_fit_specs.py,
  which replays the committed ledger and Rust byte-for-byte).

`fixtures/garmin_fit_module_absent_11_78.json` is the real capture of the
11.78 source tree (Perl 5.38.2). Set OXIDEX_ABSENT_EXIFTOOL_LIB to an
ExifTool lib without Garmin.pm (and EXIFTOOL_PERL) to recapture it live.
"""
from __future__ import annotations

import copy
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import catalog_garmin_fit
import garmin_fit_specs as specs
import join_catalog_hydrated as join

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
CAPTURE = HERE / "capture_garmin_fit_fact.pl"
ABSENT_FACT = HERE / "fixtures" / "garmin_fit_module_absent_11_78.json"
BOUNDED = HERE / "fixtures" / "garmin_fit_source.json"
LEDGER = HERE / "garmin_fit_ledger.json"
RUST = ROOT / "src" / "exiftool_tables" / "fit_tables.rs"


def perl() -> str | None:
    return os.environ.get("EXIFTOOL_PERL") or shutil.which("perl")


class PinnedTo:
    """Run the generator as if the repository pinned `version` (the rehearsal
    checkout rewrites `.exiftool-version` the same way)."""

    def __init__(self, version: str):
        self.version = version

    def __enter__(self):
        self.directory = tempfile.TemporaryDirectory(prefix="oxidex-fit-pin-")
        root = Path(self.directory.name)
        (root / ".exiftool-version").write_text(self.version + "\n")
        self.patch = patch.object(specs, "ROOT", root)
        self.patch.start()
        return self

    def __exit__(self, *exc):
        self.patch.stop()
        self.directory.cleanup()


def absent_dump(version="11.78"):
    # A dump of a release without the module: other modules, no Garmin.
    return {"exiftool_version": version, "modules": {"FITS": {"tables": {"Main": {"tags": {}}}}}}


class ModuleAbsentGenerationTests(unittest.TestCase):
    def setUp(self):
        self.fact = json.loads(ABSENT_FACT.read_text())

    def test_fixture_is_the_recorded_11_78_absence(self):
        self.assertEqual(self.fact["kind"], specs.MODULE_ABSENT_KIND)
        self.assertEqual(self.fact["native_identity"]["exiftool_version"], "11.78")
        self.assertIs(self.fact["module_file_present"], False)
        self.assertEqual(self.fact["garmin_references"], [])
        # 11.78 routes .FIT to FITS (ExifTool.pm %fileTypeLookup).
        self.assertEqual(self.fact["fit_extension_lookup"], {"alias": "FITS"})

    def test_absent_module_is_recorded_as_absent_with_zero_fit_tags(self):
        with PinnedTo("11.78"):
            rust, result = specs.generate(absent_dump(), set(), self.fact)
        self.assertTrue(specs.is_module_absent(result))
        self.assertEqual(result["module_state"], "absent")
        self.assertEqual(result["module_absence"], self.fact)
        self.assertEqual(result["protocol"]["kind"], specs.MODULE_ABSENT_KIND)
        self.assertEqual(result["protocol"]["reasons"], [])
        self.assertEqual((result["rows"], result["messages"], result["base_types"], result["tables"]),
                         ([], [], [], {}))
        self.assertEqual(result["counts"], {"source_rows": 0, "generated": 0, "refused": 0,
                                            "by_runtime_connection": {}, "refusal_reasons": {}})
        self.assertIn("refusal: Some(FitUnavailable::ModuleAbsent {", rust)
        self.assertIn('exiftool_version: "11.78"', rust)
        self.assertIn("messages: &[],", rust)
        self.assertIn("base_types: &[],", rust)
        self.assertIn("header_name: None,", rust)
        self.assertNotIn("FitField {", rust)
        self.assertNotIn("FitMessage {", rust)
        self.assertNotIn("Refused", rust)

    def test_absent_is_distinguishable_from_a_refused_protocol(self):
        document = json.loads(BOUNDED.read_text())
        document["garmin_fit_reader_protocol"]["process_fit"]["__deparse"] += "\n;"
        refused_rust, refused = specs.generate(document, set())
        self.assertFalse(specs.is_module_absent(refused))
        self.assertNotIn("module_state", refused)
        self.assertEqual(refused["protocol"]["reasons"], ["missing_or_changed_reader_protocol:ProcessFIT"])
        self.assertIn('refusal: Some(super::fit_schema::FitUnavailable::Refused('
                      '"missing_or_changed_reader_protocol:ProcessFIT"))', refused_rust)
        self.assertNotIn("ModuleAbsent", refused_rust)

    def test_bounded_projection_replays_the_absence(self):
        with PinnedTo("11.78"):
            bounded = specs.bounded_projection(absent_dump(), self.fact)
            self.assertEqual(bounded["modules"], {})
            rust, result = specs.generate(bounded, set())
            again_rust, again = specs.generate(absent_dump(), set(), self.fact)
        self.assertEqual((rust, specs.serialized(result)), (again_rust, specs.serialized(again)))

    def test_unproven_absence_is_refused(self):
        cases = {
            "release_differs_from_dump": lambda fact, dump: fact["native_identity"].update(exiftool_version="12.64"),
            "module_file_not_proven_absent": lambda fact, dump: fact.update(module_file_present=True),
            "garmin_referenced_in_source": lambda fact, dump: fact.update(
                garmin_references=[{"file": "Image/ExifTool.pm", "pattern": "table_reference"}]),
            "exiftool_module_unauthenticated": lambda fact, dump: fact["exiftool_module"].update(source_sha256="x"),
            "library_inventory_unauthenticated": lambda fact, dump: fact["library_inventory"].update(file_count=0),
            "dump_carries_the_module": lambda fact, dump: dump["modules"].update(Garmin={"tables": {}}),
            "unexpected_key:process_fit": lambda fact, dump: fact.update(process_fit={}),
            "missing_key:library_inventory": lambda fact, dump: fact.pop("library_inventory"),
        }
        for reason, mutate in cases.items():
            fact, dump = copy.deepcopy(self.fact), absent_dump()
            mutate(fact, dump)
            with self.subTest(reason), PinnedTo("11.78"), \
                    self.assertRaisesRegex(ValueError, "absence is not proven: .*" + reason):
                specs.generate(dump, set(), fact)

    def test_absent_fact_cannot_stand_in_for_the_pinned_release(self):
        # 13.59 ships the module: an absent fact relabelled 13.59 is refused
        # because the dump carries Garmin, whatever the label says.
        fact = copy.deepcopy(self.fact)
        fact["native_identity"]["exiftool_version"] = "13.59"
        with self.assertRaisesRegex(ValueError, "dump_carries_the_module"):
            specs.generate(json.loads(BOUNDED.read_text()), set(), fact)

    def test_protocol_fact_without_the_module_in_the_dump_is_refused(self):
        protocol = json.loads(BOUNDED.read_text())["garmin_fit_reader_protocol"]
        with PinnedTo("11.78"), self.assertRaisesRegex(ValueError, "no Garmin::FIT table"):
            specs.generate(absent_dump(), set(), protocol)

    def test_no_fact_without_the_module_is_refused(self):
        with PinnedTo("11.78"), self.assertRaisesRegex(ValueError, "no Garmin::FIT table"):
            specs.generate(absent_dump(), set(), None)


class CaptureFailureTests(unittest.TestCase):
    """An empty or partial capture is a failure, never an absence."""

    def test_empty_or_unparsable_fact_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            for label, text in (("empty", ""), ("blank", "\n"), ("truncated", '{\n   "kind" : "garmin_fit_mod')):
                path = Path(directory) / f"{label}.json"
                path.write_text(text)
                with self.subTest(label), self.assertRaisesRegex(ValueError, "the capture failed"):
                    specs.load_fact(path)

    def test_codegen_refuses_an_empty_fact(self):
        with tempfile.TemporaryDirectory() as directory:
            dump, fact = Path(directory) / "tables.json", Path(directory) / "fact.json"
            dump.write_text(json.dumps(absent_dump(specs.ROOT.joinpath(".exiftool-version").read_text().strip())))
            fact.write_text("")
            run = subprocess.run(
                ["python3", str(HERE / "codegen.py"), str(dump), "-o", str(Path(directory) / "out.rs"),
                 "--modules", "Garmin", "--fit-out", str(Path(directory) / "fit.rs"), "--fit-protocol-fact", str(fact)],
                cwd=HERE, capture_output=True, text=True)
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("empty Garmin FIT fact (the capture failed)", run.stderr)
            self.assertFalse((Path(directory) / "fit.rs").exists())


@unittest.skipUnless(perl(), "no perl to run capture_garmin_fit_fact.pl")
class CaptureScriptTests(unittest.TestCase):
    """capture_garmin_fit_fact.pl over synthetic trees: it records absence only
    when the tree proves it, and dies (empty stdout) otherwise."""

    EXIFTOOL_PM = ("package Image::ExifTool;\nuse vars qw($VERSION %fileTypeLookup);\n"
                   "$VERSION = '1.23';\n%fileTypeLookup = (FIT => 'FITS');\n{body}\n1;\n")

    def capture(self, files: dict[str, str]):
        with tempfile.TemporaryDirectory(prefix="oxidex-fit-lib-") as directory:
            lib = Path(directory)
            for name, text in files.items():
                (lib / name).parent.mkdir(parents=True, exist_ok=True)
                (lib / name).write_text(text)
            env = {key: value for key, value in os.environ.items() if not key.startswith("PERL5")}
            return subprocess.run([perl(), str(CAPTURE), str(lib)], capture_output=True, text=True, env=env)

    def test_tree_without_the_module_is_recorded_absent(self):
        run = self.capture({"Image/ExifTool.pm": self.EXIFTOOL_PM.format(body=""),
                            "Image/ExifTool/FITS.pm": "package Image::ExifTool::FITS;\nsub ProcessFITS { }\n1;\n"})
        self.assertEqual(run.returncode, 0, run.stderr)
        fact = json.loads(run.stdout)
        self.assertEqual(fact["kind"], specs.MODULE_ABSENT_KIND)
        self.assertEqual(fact["native_identity"]["exiftool_version"], "1.23")
        self.assertEqual(fact["library_inventory"]["file_count"], 2)
        self.assertEqual(fact["fit_extension_lookup"], {"alias": "FITS"})
        with PinnedTo("1.23"):
            _, result = specs.generate(absent_dump("1.23"), set(), fact)
        self.assertTrue(specs.is_module_absent(result))

    def test_garmin_reference_without_the_module_file_dies(self):
        for label, body in (("table", "my @t = qw(Garmin::FIT);"), ("reader", "sub x { ProcessFIT() }"),
                            ("module_name", "my %m = (FIT => 'Garmin');")):
            run = self.capture({"Image/ExifTool.pm": self.EXIFTOOL_PM.format(body=body)})
            with self.subTest(label):
                self.assertNotEqual(run.returncode, 0)
                self.assertEqual(run.stdout, "")
                self.assertIn("Garmin module file absent but referenced", run.stderr)

    def test_module_that_fails_to_load_dies(self):
        run = self.capture({"Image/ExifTool.pm": self.EXIFTOOL_PM.format(body=""),
                            "Image/ExifTool/Garmin.pm": "package Image::ExifTool::Garmin;\ndie 'broken';\n"})
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(run.stdout, "")


class CatalogReplayTests(unittest.TestCase):
    def test_absent_module_replays_to_zero_catalog_rows(self):
        fact = json.loads(ABSENT_FACT.read_text())
        with PinnedTo("11.78"):
            rust, result = specs.generate(absent_dump(), set(), fact)
            bounded = json.dumps(specs.bounded_projection(absent_dump(), fact)).encode()
            sources = {"Image/ExifTool.pm": {"sha256": fact["exiftool_module"]["source_sha256"]}}
            kwargs = dict(dump_source=json.dumps(absent_dump()).encode(), expr_ledger=b"{}",
                          protocol_fact=json.dumps(fact).encode(), rust_matches=join.quicktime_rust_matches)
            with patch.object(catalog_garmin_fit.codegen, "load_oracle_ledger", return_value=set()):
                rows = catalog_garmin_fit.implementation(json.loads(specs.serialized(result)), bounded, rust,
                                                         catalog_sources=sources, **kwargs)
                self.assertEqual(rows, {})
                wrong = {"Image/ExifTool.pm": {"sha256": "0" * 64}}
                with self.assertRaisesRegex(ValueError, "differs from catalog provenance"):
                    catalog_garmin_fit.implementation(json.loads(specs.serialized(result)), bounded, rust,
                                                      catalog_sources=wrong, **kwargs)


class PinnedReleaseUnchangedTests(unittest.TestCase):
    def test_pinned_release_is_present_and_admitted(self):
        ledger = json.loads(LEDGER.read_text())
        self.assertNotIn("module_state", ledger)
        self.assertEqual(ledger["protocol"]["kind"], specs.PROTOCOL_KIND)
        self.assertTrue(ledger["protocol"]["admitted"])
        rust = RUST.read_text()
        self.assertIn("    refusal: None,\n", rust)
        self.assertNotIn("FitUnavailable", rust)
        # And the committed artifacts replay byte-for-byte from the bounded
        # source through the changed generator.
        replay_rust, replay = specs.generate(json.loads(BOUNDED.read_text()),
                                             specs.unbound_verified_expressions(HERE / "expr_oracle_ledger.json"))
        self.assertEqual(specs.serialized(replay), LEDGER.read_text())
        self.assertTrue(join.quicktime_rust_matches(replay_rust, rust))


class LiveAbsentCaptureTests(unittest.TestCase):
    def test_recapture_matches_the_fixture(self):
        lib = os.environ.get("OXIDEX_ABSENT_EXIFTOOL_LIB")
        if not lib or not os.environ.get("EXIFTOOL_PERL"):
            self.skipTest("set OXIDEX_ABSENT_EXIFTOOL_LIB and EXIFTOOL_PERL to recapture 11.78")
        run = subprocess.run([os.environ["EXIFTOOL_PERL"], str(CAPTURE), lib], capture_output=True, text=True)
        self.assertEqual(run.returncode, 0, run.stderr)
        fact, fixture = json.loads(run.stdout), json.loads(ABSENT_FACT.read_text())
        if fact["native_identity"]["exiftool_version"] != "11.78":
            self.skipTest("OXIDEX_ABSENT_EXIFTOOL_LIB is not the 11.78 tree")
        self.assertEqual(fact, fixture)


if __name__ == "__main__":
    unittest.main()
