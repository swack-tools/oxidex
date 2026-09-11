"""Exercise the GeoTIFF CLI and independent compiled-fact verifier."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PERL = shutil.which("perl")


@unittest.skipUnless(PERL and shutil.which("rustfmt") and shutil.which("rustc"),
                     "GeoTIFF controls require Perl, rustfmt and rustc")
class GeoTiffProducer(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="geotiff-controls-")
        self.addCleanup(self.scratch.cleanup)
        self.base = Path(self.scratch.name)
        self.tree = self.base / "source"
        self.module = self.tree / "lib/Image/ExifTool/GeoTiff.pm"
        self.module.parent.mkdir(parents=True)
        self.core = self.tree / "lib/Image/ExifTool.pm"
        self.core.write_text("package Image::ExifTool; our $VERSION = '" +
                             (ROOT / ".exiftool-version").read_text().strip() + "'; 1;\n")
        self.body = """package Image::ExifTool::GeoTiff;
use Image::ExifTool;
%Image::ExifTool::GeoTiff::Main = (
    GROUPS => { 2 => 'Location' },
    1024 => { Name => 'GTModelType', PrintConv => { 1 => 'Projected', 2 => 'Geographic' } },
    1025 => { Name => 'GTRasterType', PrintConv => { 1 => 'Pixel Area', 2 => 'Pixel Point' } },
    1026 => 'GTCitation',
);
1;
"""
        self.module.write_text(self.body)
        self.output = self.base / "candidate.rs"
        self.env = os.environ.copy()

    def cli(self, *args, verifier=False, ok=True):
        argv = [sys.executable, str(HERE / ("verify_geotiff.py" if verifier else
                                           "gen_geotiff_printconv.py")),
                "--exiftool-dir", str(self.tree), "--perl", PERL,
                "--rust-file" if verifier else "--out", str(self.output), *args]
        result = subprocess.run(argv, text=True, capture_output=True, env=self.env)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        return result

    def unchanged_refusal(self, body=None):
        if body is not None:
            self.module.write_text(body)
        self.output.write_text("KEEP EXISTING OUTPUT\n")
        self.output.chmod(0o640)
        self.cli(ok=False)
        self.assertEqual(self.output.read_text(), "KEEP EXISTING OUTPUT\n")
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o640)
        self.assertEqual(list(self.base.glob(".geotiff-*")), [])

    def test_generate_check_and_complete_independent_facts(self):
        self.cli()
        first = self.output.read_bytes()
        self.output.chmod(0o640)
        self.cli()
        self.assertEqual(self.output.read_bytes(), first)
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o640)
        self.cli("--check")
        report = json.loads(self.cli(verifier=True).stdout.splitlines()[-1])
        self.assertTrue(report["passed"])
        self.assertEqual((report["names"], report["conversion_keys"], report["conversion_facts"]),
                         (3, 2, 4))

    def test_check_never_writes_missing_or_stale_output(self):
        self.cli("--check", ok=False)
        self.assertFalse(self.output.exists())
        self.output.write_text("stale")
        self.cli("--check", ok=False)
        self.assertEqual(self.output.read_text(), "stale")

    def test_explicit_perl_ignores_hostile_path_and_perl_environment(self):
        fakebin = self.base / "bin"
        fakebin.mkdir()
        fakeperl = fakebin / "perl"
        fakeperl.write_text("#!/bin/sh\nexit 98\n")
        fakeperl.chmod(0o755)
        self.env.update(PATH=str(fakebin) + os.pathsep + os.environ["PATH"],
                        PERL5OPT="-MThisModuleMustNotLoad", PERL5LIB="/absent", PERLLIB="/absent")
        self.cli()
        self.cli(verifier=True)

    def test_wrong_version_cannot_be_hidden_by_cache_stamp(self):
        self.core.write_text("package Image::ExifTool; our $VERSION = '0.01'; 1;\n")
        (self.tree / ".version").write_text((ROOT / ".exiftool-version").read_text())
        self.unchanged_refusal()
        self.cli(verifier=True, ok=False)

    def test_missing_selected_module_cannot_fall_back_to_environment(self):
        ambient = self.base / "ambient"
        shutil.copytree(self.tree / "lib", ambient)
        self.module.unlink()
        self.env["PERL5LIB"] = str(ambient)
        self.unchanged_refusal()
        self.cli(verifier=True, ok=False)

    def test_unmodeled_fields_and_keys_refuse_without_writing(self):
        for edit in ("99999 => 'Overflow',", "-1 => 'Negative',", "1.5 => 'Fraction',",
                     "UNKNOWN => 'Metadata',", "'010' => 'Noncanonical',"):
            with self.subTest(edit=edit):
                self.unchanged_refusal(self.body.replace("    GROUPS", "    " + edit + "\n    GROUPS"))
        self.unchanged_refusal(self.body.replace("Name => 'GTModelType',", "Name => 'GTModelType', Mask => 1,"))

    def test_unmodeled_conversion_values_refuse_without_writing(self):
        for replacement in ("undef", "{}", "{ 1 => undef }", "{ 1 => sub { 1 } }",
                            "{ -1 => 'Negative' }", "{ 65536 => 'Overflow' }",
                            "{ OTHER => 'Other' }", "{ 1 => '' }", "{ 1 => \"bad\\nvalue\" }"):
            with self.subTest(replacement=replacement):
                self.unchanged_refusal(self.body.replace("{ 1 => 'Projected', 2 => 'Geographic' }", replacement))

    def test_formatter_failure_preserves_output(self):
        fakebin = self.base / "bin"
        fakebin.mkdir()
        formatter = fakebin / "rustfmt"
        formatter.write_text("#!/bin/sh\nexit 98\n")
        formatter.chmod(0o755)
        self.env["PATH"] = str(fakebin) + os.pathsep + os.environ["PATH"]
        self.unchanged_refusal()

    def test_nonregular_output_refuses_without_replacing_it(self):
        target = self.base / "target"
        target.write_text("preserve target")
        self.output.symlink_to(target)
        self.cli(ok=False)
        self.assertTrue(self.output.is_symlink())
        self.assertEqual(target.read_text(), "preserve target")
        self.output.unlink()
        self.output.mkdir()
        self.cli(ok=False)
        self.assertTrue(self.output.is_dir())

    def test_empty_or_names_only_population_refuses(self):
        for entries in ("", "1024 => 'GTModelType',"):
            with self.subTest(entries=entries):
                body = ("package Image::ExifTool::GeoTiff; use Image::ExifTool;\n"
                        "%Image::ExifTool::GeoTiff::Main = (\n" + entries + "\n);\n1;\n")
                self.unchanged_refusal(body)
                self.cli(verifier=True, ok=False)

    def test_independent_verifier_detects_value_name_missing_extra_and_dispatch_drift(self):
        self.cli()
        original = self.output.read_text()
        mutations = {
            "value": original.replace('"Projected"', '"WRONG"'),
            "name": original.replace('"GTModelType"', '"WRONG"'),
            "missing": original.replace(', (2, "Geographic")', ''),
            "extra": original.replace('(2, "Geographic")', '(2, "Geographic"), (3, "Unexpected")'),
            "dispatch": original.replace('1024 => Some(GT_MODEL_TYPE)', '1024 => Some(GT_RASTER_TYPE)'),
            "empty-dispatch": original.replace('1024 => Some(GT_MODEL_TYPE)', '1024 => Some(&[])'),
            "lookup": original.replace('.map(|i| map[i].1)', '.map(|_i| "WRONG")'),
        }
        for kind, candidate in mutations.items():
            with self.subTest(kind=kind):
                self.assertNotEqual(candidate, original)
                self.output.write_text(candidate)
                self.cli(verifier=True, ok=False)


if __name__ == "__main__":
    unittest.main()
