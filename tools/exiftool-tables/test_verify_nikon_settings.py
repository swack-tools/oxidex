"""Independent CLI controls: tiny real Perl tables, never generator output."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "verify_nikon_settings.py"
PERL = shutil.which("/usr/bin/perl") or shutil.which("perl")
PIN = (HERE.parents[1] / ".exiftool-version").read_text().strip()
TABLE = r'''package Image::ExifTool::NikonSettings;
use strict; use warnings;
sub ProcessNikonSettings { }
our %Main = (
  PROCESS_PROC => \&ProcessNikonSettings,
  GROUPS => {0 => 'MakerNotes', 2 => 'Camera'}, NOTES => 'Fixture',
  1 => [
    {Name=>'Choice', Condition=>'$$self{Model} =~ /^NIKON D6\\b/i', PrintConv=>{0=>'Off',1=>'On'}},
    {Name=>'Choice', PrintConv=>{0=>'No',1=>'Yes'}},
  ],
  2 => {Name=>'Fine', ValueConv=>'($val - 7) / 6', PrintConv=>'$val ? sprintf("%+.2f", $val) : 0'},
  3 => {Name=>'Rate', ValueConv=>'6 - $val', PrintConv=>'"$val fps"'},
  4 => {Name=>'Count', ValueConv=>'10 - $val'},
  5 => {Name=>'Mode', RawConv=>'$$self{BracketSet} = $val', PrintConv=>{0=>'Zero',1=>'One'}},
  6 => {Name=>'Hidden', Unknown=>1, RawConv=>'$$self{CmdDialsChangeMainSubExposure} = $val'},
  7 => {Name=>'Offset', PrintConv=>'$val-6'},
  8 => {Name=>'Number'},
  366 => {Name=>'AFAreaMode', RawConv=>'$$self{AFAreaMode} = $val', PrintConv=>{2=>'Single-point'}},
);
1;
'''
RUST = r'''//! Fixture header.
use super::settings::{Cond, Conv, Dm, SettingsTag as E};
#[rustfmt::skip]
const PC_0: &[(u32, &str)] = &[(0, "Off"), (1, "On")];
const PC_1: &[(u32, &str)] = &[(0, "No"), (1, "Yes")];
const PC_2: &[(u32, &str)] = &[(0, "Zero"), (1, "One")];
const PC_3: &[(u32, &str)] = &[(2, "Single-point")];
pub(super) const SETTINGS_TAGS: &[E] = &[
 E { id: 1, name: "Choice", cond: Cond::ModelD6, mask: 0, conv: Conv::Map(PC_0), dm: Dm::None },
 E { id: 1, name: "Choice", cond: Cond::Always, mask: 0, conv: Conv::Map(PC_1), dm: Dm::None },
 E { id: 2, name: "Fine", cond: Cond::Always, mask: 0, conv: Conv::FineTune, dm: Dm::None },
 E { id: 3, name: "Rate", cond: Cond::Always, mask: 0, conv: Conv::Fps(6), dm: Dm::None },
 E { id: 366, name: "AFAreaMode", cond: Cond::Always, mask: 0, conv: Conv::Map(PC_3), dm: Dm::None },
 E { id: 4, name: "Count", cond: Cond::Always, mask: 0, conv: Conv::Negate(10), dm: Dm::None },
 E { id: 5, name: "Mode", cond: Cond::Always, mask: 0, conv: Conv::Map(PC_2), dm: Dm::BracketSet },
 E { id: 7, name: "Offset", cond: Cond::Always, mask: 0, conv: Conv::Offset(-6), dm: Dm::None },
 E { id: 8, name: "Number", cond: Cond::Always, mask: 0, conv: Conv::Raw, dm: Dm::None },
];
'''

@unittest.skipUnless(PERL, "Perl with JSON::PP is required")
class NikonSettingsVerifierTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "source"
        self.pm = self.source / "lib/Image/ExifTool/NikonSettings.pm"
        self.pm.parent.mkdir(parents=True)
        self.core = self.source / "lib/Image/ExifTool.pm"
        self.core.write_text(f"package Image::ExifTool; our $VERSION = '{PIN}'; 1;\n")
        self.pm.write_text(TABLE)
        self.output = self.root / "settings.rs"
        self.output.write_text(RUST)
        self.env = {k:v for k,v in os.environ.items() if not k.startswith("PERL")}
        self.env["PYTHONDONTWRITEBYTECODE"] = "1"

    def run_cli(self, good=False):
        before = self.output.read_bytes()
        result = subprocess.run([sys.executable, str(SCRIPT), "--exiftool-dir", str(self.source),
                                 "--perl", PERL, "--input", str(self.output)],
                                capture_output=True, text=True, env=self.env, timeout=15)
        self.assertEqual(self.output.read_bytes(), before)
        self.assertEqual(result.stdout.splitlines()[0], '=== instrument: verify_nikon_settings.py ===')
        if good:
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout.splitlines()[-1])
            self.assertEqual(report['instrument'], 'verify_nikon_settings.py')
            self.assertEqual(report['version'], PIN)
            return report
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('Nikon settings verification refused:', result.stderr)
        return result

    def test_all_forms_and_explicit_state_residual(self):
        result = self.run_cli(good=True)
        self.assertEqual(result["rows"], 9)
        self.assertEqual(result["unknown_omitted"], 1)
        self.assertEqual(result["unpropagated_state"], [{"id": 366, "name": "AFAreaMode", "member": "AFAreaMode"}])

    def test_changed_rust_facts_fail(self):
        for old,new in [('name: "Fine"','name: "Finer"'),("Cond::ModelD6","Cond::ModelZ7"),
                        ("mask: 0","mask: 1"),("Conv::Fps(6)","Conv::Fps(7)"),
                        ("Conv::Offset(-6)","Conv::Offset(6)"),("Dm::BracketSet","Dm::BracketProgram"),
                        ('(1, "On")','(1, "Wrong")')]:
            with self.subTest(new=new):
                self.output.write_text(RUST.replace(old,new,1)); self.run_cli()

    def test_missing_extra_and_reordered_variants_fail(self):
        row = next(x for x in RUST.splitlines(True) if 'Cond::ModelD6' in x)
        fallback = next(x for x in RUST.splitlines(True) if 'name: "Choice"' in x and 'Cond::Always' in x)
        for text in [RUST.replace(row,''),RUST.replace(row,row+row),
                     RUST.replace(row+fallback,fallback+row)]:
            with self.subTest(text=text):
                self.output.write_text(text); self.run_cli()

    def test_commented_or_string_contained_data_is_not_a_row(self):
        row = next(x for x in RUST.splitlines(True) if 'name: "Number"' in x)
        for replacement in ['/*'+row+'*/', '//'+row, json.dumps(row)+';\n']:
            self.output.write_text(RUST.replace(row,replacement)); self.run_cli()

    def test_unparsed_or_duplicate_declarations_fail(self):
        for extra in ['const PC_99: &[(u32,&str)] = &[(1,"Unused")];',
                      'fn ignored() {}',RUST, '/* unclosed', '/* outer /* inner */',
                      '#[rustfmt::skip]']:
            self.output.write_text(RUST+extra); self.run_cli()

    def test_new_native_read_fields_and_changed_semantics_fail(self):
        for old,new in [("Name=>'Fine'","Name=>'Fine',Format=>'int16u'"),
                        ("Name=>'Fine'","Name=>'Fine',Hook=>'die'"),
                        ("6 - $val","7 - $val"),
                        ("$$self{AFAreaMode} = $val","$$self{AFAreaMode} = 1 + $val"),
                        ("Unknown=>1","Unknown=>0")]:
            self.pm.write_text(TABLE.replace(old,new)); self.run_cli()

    def test_empty_or_malformed_native_table_fails(self):
        for table in [TABLE.replace("0=>'Off'","0=>[]"),TABLE.replace("Name=>'Fine'","Name=>[]"),
                      "package Image::ExifTool::NikonSettings; our %Main; 1;"]:
            self.pm.write_text(table); self.run_cli()

    def test_source_pin_and_loaded_path_must_match(self):
        self.core.write_text("package Image::ExifTool; our $VERSION='0'; 1;\n"); self.run_cli()
        self.core.write_text(f"package Image::ExifTool; our $VERSION='{PIN}'; 1;\n")
        self.pm.write_text(TABLE+'\n$INC{"Image/ExifTool/NikonSettings.pm"}="elsewhere";\n'); self.run_cli()

    def test_matching_but_perl_false_names_still_refuse(self):
        for name in ('', '0'):
            self.pm.write_text(TABLE.replace("Name=>'Fine'", f"Name=>'{name}'"))
            self.output.write_text(RUST.replace('name: "Fine"', f'name: "{name}"'))
            self.run_cli()

    def test_missing_selected_module_refuses_ambient_fallback(self):
        ambient = self.root/'ambient/Image/ExifTool'; ambient.mkdir(parents=True)
        (ambient/'NikonSettings.pm').write_text(TABLE)
        self.pm.unlink(); self.env['PERL5LIB']=str(self.root/'ambient'); self.run_cli()

    def test_hostile_perl_environment_does_not_preload(self):
        self.env['PERL5OPT']='-MThisModuleMustNotLoad'
        self.env['PERL5LIB']=str(self.root/'wrong')
        self.run_cli(good=True)

    def test_literal_whitespace_and_escapes_are_exact(self):
        self.pm.write_text(TABLE.replace("0=>'Off'",r"0=>'Off  // \" \\ end'".replace(r'\"', '"')))
        self.output.write_text(RUST.replace('"Off"',r'"Off  // \" \\ end"'))
        self.run_cli(good=True)
        self.output.write_text(self.output.read_text().replace('Off  //','Off //'))
        self.run_cli()

    def test_mixed_unknown_variant_veto_is_not_filtered_away(self):
        self.pm.write_text(TABLE.replace("{Name=>'Choice', Condition", "{Unknown=>1, Name=>'Choice', Condition"))
        row = next(x for x in RUST.splitlines(True) if 'Cond::ModelD6' in x)
        table = next(x for x in RUST.splitlines(True) if x.startswith('const PC_0:'))
        self.output.write_text(RUST.replace(row, '').replace(table, ''))
        self.run_cli()

    def test_new_mask_cannot_be_certified_as_a_native_read_operation(self):
        self.pm.write_text(TABLE.replace("Name=>'Fine'", "Name=>'Fine',Mask=>1"))
        self.output.write_text(RUST.replace('name: "Fine", cond: Cond::Always, mask: 0',
                                            'name: "Fine", cond: Cond::Always, mask: 1'))
        self.run_cli()

    def test_unknown_condition_is_checked_before_omission(self):
        self.pm.write_text(TABLE.replace("Name=>'Hidden'", "Name=>'Hidden', Condition=>'$$self{BracketSet} < 4'"))
        self.run_cli(good=True)
        self.pm.write_text(TABLE.replace("Name=>'Hidden'", "Name=>'Hidden', Condition=>'$$self{BracketSet} = 1'"))
        self.run_cli()

    def test_known_mask_is_reported_as_runtime_residual(self):
        self.pm.write_text(TABLE.replace("  3 =>", "  266 => {Name=>'BracketProgram', Condition=>'$$self{BracketSet} and $$self{BracketSet} == 5', Mask=>15, RawConv=>'$$self{BracketProgram} = $val', PrintConv=>{0=>'Off',1=>'On'}},\n  3 =>"))
        self.output.write_text(RUST.replace(' E { id: 3,', ' E { id: 266, name: "BracketProgram", cond: Cond::BracketSetIs(5), mask: 15, conv: Conv::Map(PC_0), dm: Dm::BracketProgram },\n E { id: 3,'))
        result = self.run_cli(good=True)
        self.assertEqual(result['runtime_mask_residual'], [{'id': 266, 'name': 'BracketProgram', 'mask': 15}])

if __name__ == '__main__':
    unittest.main()
