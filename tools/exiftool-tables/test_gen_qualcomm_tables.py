"""Controls for scripts/gen_qualcomm_tables.pl's Qualcomm::Main VARS guard.

The guard admits only VARS keys proven, from ExifTool's own consumers, to
affect documentation / TagLookup.pm generation and never what is read:
NO_LOOKUP, and the Tag-ID-column switch spelled NO_ID => 1 (11.78, 12.64) or
ID_FMT => 'none' (13.x). Any other key or value still refuses.

The synthetic-library tests are hermetic (core Perl only). The last test
regenerates from the gate-provided pinned tree and requires byte identity with
the committed artifact.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
GENERATOR = ROOT / "scripts" / "gen_qualcomm_tables.pl"
PIN_MODULE = ROOT / "scripts" / "lib" / "ExiftoolPin.pm"
COMMITTED = ROOT / "src" / "parsers" / "jpeg" / "app_segments" / "qualcomm_tables.rs"
PERL = os.environ.get("EXIFTOOL_PERL") or shutil.which("perl") or "/usr/bin/perl"

EXIFTOOL_PM = r"""package Image::ExifTool;
use strict;
use vars qw($VERSION);
$VERSION = '13.59';
sub DirStart { }
sub dispatch {
    my ($segDataPt) = @_;
    my %dirInfo;
    if ($$segDataPt =~ /^\x1aQualcomm Camera Attributes/) {
        DirStart(\%dirInfo, 27);
    }
}
1;
"""

QUALCOMM_PM = r"""package Image::ExifTool::Qualcomm;
use strict;
use vars qw($VERSION);
$VERSION = '1.02';
my @qualcommFormat = (
    'int8u',    'int8s',    'int16u',   'int16s',
    'int32u',   'int32s',   'float',    'double',
);
%Image::ExifTool::Qualcomm::Main = (
    GROUPS => { 0 => 'MakerNotes', 2 => 'Camera' },
    VARS => @VARS@,
    NOTES => q{ synthetic },
    'af_position' => { },
    'aec_current_exp_index' => { },
);
my $t = \%Image::ExifTool::Qualcomm::Main;
$$t{af_position} = { Name => 'AFPosition', Description => 'AF Position' };
$$t{aec_current_exp_index} = { Name => 'AECCurrentExpIndex', Description => 'AEC Current Exp Index' };
1;
"""


def run_synthetic(vars_literal):
    """Run the real generator against a synthetic 13.59-pinned tree."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / ".exiftool-version").write_text("13.59\n")
        (root / "scripts" / "lib").mkdir(parents=True)
        shutil.copy(GENERATOR, root / "scripts" / GENERATOR.name)
        shutil.copy(PIN_MODULE, root / "scripts" / "lib" / PIN_MODULE.name)
        lib = root / "et" / "lib"
        (lib / "Image" / "ExifTool").mkdir(parents=True)
        (lib / "Image" / "ExifTool.pm").write_text(EXIFTOOL_PM)
        (lib / "Image" / "ExifTool" / "Qualcomm.pm").write_text(
            QUALCOMM_PM.replace("@VARS@", vars_literal))
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(("PERL5", "PERLLIB"))}
        env["OXIDEX_EXIFTOOL_LIB"] = str(lib)
        return subprocess.run([PERL, str(root / "scripts" / GENERATOR.name)],
                              capture_output=True, text=True, env=env, check=False)


class QualcommVarsGuard(unittest.TestCase):
    def assert_admitted(self, vars_literal):
        result = run_synthetic(vars_literal)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('("af_position", "AFPosition", "AF Position"),', result.stdout)
        self.assertIn("NAME_FIXTURE: [(&str, &str, &str); 2]", result.stdout)
        return result.stdout

    def assert_refused(self, vars_literal, reason):
        result = run_synthetic(vars_literal)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(result.stdout, "")
        self.assertIn("REFUSING: unexpected VARS in Qualcomm::Main", result.stderr)
        self.assertIn(reason, result.stderr)

    def test_13x_spelling_admitted(self):
        self.assert_admitted("{ ID_FMT => 'none', NO_LOOKUP => 1 }")

    def test_pre_13_no_id_spelling_admitted_with_identical_output(self):
        # 11.78 and 12.64 declare NO_ID => 1 where 13.x declares ID_FMT =>
        # 'none'. Both only hide the doc Tag ID column, so the emitted reader
        # must be byte-identical.
        old = self.assert_admitted("{ NO_ID => 1, NO_LOOKUP => 1 }")
        new = self.assert_admitted("{ ID_FMT => 'none', NO_LOOKUP => 1 }")
        self.assertEqual(old, new)

    def test_unrelated_var_still_refused(self):
        self.assert_refused("{ ID_FMT => 'none', NO_LOOKUP => 1, ALLOW_REPROCESS => 1 }",
                            "'ALLOW_REPROCESS' is not a proven documentation-only VARS key")
        self.assert_refused("{ NO_ID => 1, NO_LOOKUP => 1, START_INDEX => 1 }",
                            "'START_INDEX' is not a proven documentation-only VARS key")

    def test_unproven_value_refused(self):
        self.assert_refused("{ ID_FMT => 'dec', NO_LOOKUP => 1 }",
                            "'ID_FMT' has a value this generator has not proven")
        self.assert_refused("{ NO_ID => 0, NO_LOOKUP => 1 }",
                            "'NO_ID' has a value this generator has not proven")

    def test_changed_shape_refused(self):
        expected = "expected NO_LOOKUP plus exactly one of ID_FMT='none' / NO_ID"
        self.assert_refused("{ ID_FMT => 'none' }", expected)
        self.assert_refused("{ NO_LOOKUP => 1 }", expected)
        self.assert_refused("{ ID_FMT => 'none', NO_ID => 1, NO_LOOKUP => 1 }", expected)

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                         "requires gate-provided EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
    def test_pinned_release_regenerates_committed_artifact_byte_identically(self):
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(("PERL5", "PERLLIB"))}
        env["OXIDEX_EXIFTOOL_LIB"] = str(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]) / "lib")
        result = subprocess.run([os.environ["EXIFTOOL_PERL"], str(GENERATOR)],
                                capture_output=True, env=env, check=False)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(result.stdout, COMMITTED.read_bytes())


if __name__ == "__main__":
    unittest.main()
