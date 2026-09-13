"""Full CLI controls with real Perl fixture tables, independent of the producer."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / 'verify_sony_plain.py'
PIN = (HERE.parents[1] / '.exiftool-version').read_text().strip()
PERL = shutil.which('/usr/bin/perl') or shutil.which('perl')
NAMES = ['CameraSettings', 'CameraSettings2', 'CameraSettings3', 'FaceInfo1', 'FaceInfo2', 'ShotInfo']
PERL_ROWS = [
    '''0 => {Name=>'Choice', PrintConv=>{0=>'Off',1=>'On', BITMASK=>{0=>'Bit'}}},
       1 => {Name=>'ExposureTime',ValueConv=>'$val ? 2 ** (6 - $val/8) : 0',PrintConv=>'$val ? Image::ExifTool::Exif::PrintExposureTime($val) : "Bulb"'},''',
    '''0 => {Name=>'Comp', Format=>'int16u[3]', ValueConv=>'($val - 128) / 24', PrintConv=>'$val ? sprintf("%+.1f",$val) : 0'},''',
    '''276 => {Name=>'FolderNumber',Condition=>'$$self{Model} !~ /^DSLR-(A450|A500|A550)$/',Format=>'int32u',Mask=>16760832,PrintConv=>'sprintf("%.3d",$val)'},
       '276.1' => {Name=>'ImageNumber',Condition=>'$$self{Model} !~ /^DSLR-(A450|A500|A550)$/',Format=>'int32u',Mask=>16383,PrintConv=>'sprintf("%.4d",$val)'},
       1015 => {Name=>'LensType2',Condition=>'($$self{Model} =~ /^NEX-/) and ($$self{LensMount} != 1)',Format=>'int16u',PrintInt=>1,PrintConv=>{0=>'Lens', '0.1'=>'Alternative'}},''',
    '''0 => {Name=>'Face',Format=>'int16u[4]',RawConv=>'$$self{FacesDetected} < 1 ? undef : $val'},''',
    "0 => {Name=>'OtherFace',PrintConv=>{0=>'Never',OTHER=>sub {shift}}},",
    '''2 => {Name=>'FaceInfoOffset',Format=>'int16u',DataMember=>'FaceInfoOffset',RawConv=>'$$self{FaceInfoOffset} = $val'},
       72 => {Name=>'FaceInfo1',Condition=>'$$self{FacesDetected} and $$self{FaceInfoOffset} == 0x48 and $$self{FaceInfoLength} == 0x20',SubDirectory=>{TagTable=>'Image::ExifTool::Sony::FaceInfo1'}},''',
]


def perl_fixture():
    text = 'package Image::ExifTool::Sony; use strict; use warnings;\n'
    for i, name in enumerate(NAMES):
        meta = "PROCESS_PROC=>\\&Image::ExifTool::ProcessBinaryData, CHECK_PROC=>\\&Image::ExifTool::CheckBinaryData, WRITE_PROC=>\\&Image::ExifTool::WriteBinaryData, WRITABLE=>1, FIRST_ENTRY=>0,"
        meta += "GROUPS=>{0=>'MakerNotes',2=>'" + ('Camera' if i < 3 else 'Image') + "'},"
        if i < 3:
            meta += "PRIORITY=>0, FORMAT=>'" + ('int16u' if i < 2 else 'int8u') + "',"
        if i == 5:
            meta += 'DATAMEMBER=>[2], IS_SUBDIR=>[72],'
        text += f'our %{name} = ({meta}\n{PERL_ROWS[i]});\n'
    return text + '1;\n'


RUST_ROWS = [
    '''BinTag { index: 0, name: "Choice", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Bitmask(M0, B0, 32, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
BinTag { index: 1, name: "ExposureTime", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::ExpTime(6.0_f64, 8.0_f64), pc: Pc::ExposureTimeOrBulb, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },''',
    '''BinTag { index: 0, name: "Comp", cond: Cond::Always, fmt: Fmt::U16, count: 3, mask: 0, raw: Raw::None, vc: Vc::SubDiv(128.0_f64, 24.0_f64), pc: Pc::Signed1OrZero, hook: Hook::None, print_hex: false, low_priority: true, subdir: None },''',
    '''BinTag { index: 276, name: "FolderNumber", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U32, count: 1, mask: 16760832, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(3), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
BinTag { index: 276, name: "ImageNumber", cond: Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$"), fmt: Fmt::U32, count: 1, mask: 16383, raw: Raw::None, vc: Vc::None, pc: Pc::ZeroPad(4), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },
BinTag { index: 1015, name: "LensType2", cond: Cond::All(&[Cond::ModelRe(false, r"^NEX-"), Cond::DmCmp(Dm::LensMount, NumCmp::Ne, 1.0_f64)]), fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M1, Other::None), hook: Hook::None, print_hex: false, low_priority: true, subdir: None },''',
    '''BinTag { index: 0, name: "Face", cond: Cond::Always, fmt: Fmt::U16, count: 4, mask: 0, raw: Raw::DropIfDmLess(Dm::FacesDetected, 1.0_f64), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },''',
    '''BinTag { index: 0, name: "OtherFace", cond: Cond::Always, fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::Map(M2, Other::Identity), hook: Hook::None, print_hex: false, low_priority: false, subdir: None },''',
    '''BinTag { index: 2, name: "FaceInfoOffset", cond: Cond::Always, fmt: Fmt::U16, count: 1, mask: 0, raw: Raw::Store(Dm::FaceInfoOffset), vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: None },
BinTag { index: 72, name: "FaceInfo1", cond: Cond::All(&[Cond::DmTruthy(Dm::FacesDetected), Cond::DmCmp(Dm::FaceInfoOffset, NumCmp::Eq, 72.0_f64), Cond::DmCmp(Dm::FaceInfoLength, NumCmp::Eq, 32.0_f64)]), fmt: Fmt::Default, count: 1, mask: 0, raw: Raw::None, vc: Vc::None, pc: Pc::None, hook: Hook::None, print_hex: false, low_priority: false, subdir: Some(3) },''',
]


def rust_fixture():
    text = '''use super::binary_data::{BinTable, BinTag, Cond, Dm, Fmt, Hook, NumCmp, Other, Pc, Raw, Vc};
#[rustfmt::skip]
static M0: &[(&str, &str)] = &[("0", "Off"), ("1", "On")];
static M1: &[(&str, &str)] = &[("0", "Lens"), ("0.1", "Alternative")];
static M2: &[(&str, &str)] = &[("0", "Never")];
static B0: &[(u32, &str)] = &[(0u32, "Bit")];
'''
    for i, rows in enumerate(RUST_ROWS):
        text += f'static T{i}: &[BinTag] = &[\n{rows}\n];\n'
    text += 'pub static TABLES: &[BinTable] = &[\n'
    for i, name in enumerate(NAMES):
        fmt = ['U16', 'U16', 'U8', 'Default', 'Default', 'Default'][i]
        text += f'BinTable {{ name: "{name}", fmt: Fmt::{fmt}, tags: T{i} }},\n'
    text += '];\n#[allow(dead_code)]\npub mod idx {\n'
    for i, name in enumerate(NAMES):
        text += f'pub const {name.upper()}: usize = {i};\n'
    return text + '}\n'


@unittest.skipUnless(PERL, 'Perl required for live fixture tables')
class SonyPlainVerifierTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.source = self.base / 'source'
        self.pm = self.source / 'lib/Image/ExifTool/Sony.pm'
        self.pm.parent.mkdir(parents=True)
        self.core = self.source / 'lib/Image/ExifTool.pm'
        self.core.write_text(f"package Image::ExifTool; our $VERSION='{PIN}'; sub ProcessBinaryData{{}} sub CheckBinaryData{{}} sub WriteBinaryData{{}} 1;\n")
        self.pm.write_text(perl_fixture())
        self.rust = self.base / 'plain.rs'
        self.rust.write_text(rust_fixture())
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('PERL')}
        self.env['PYTHONDONTWRITEBYTECODE'] = '1'

    def cli(self, good=False):
        original = self.rust.read_bytes()
        p = subprocess.run([sys.executable, str(SCRIPT), '--exiftool-dir', str(self.source), '--perl', PERL,
                            '--input', str(self.rust)], env=self.env, capture_output=True, text=True, timeout=20)
        self.assertEqual(self.rust.read_bytes(), original)
        self.assertEqual(p.stdout.splitlines()[:1], ['=== instrument: verify_sony_plain.py ==='], p.stderr)
        if good:
            self.assertEqual(p.returncode, 0, p.stderr)
            return json.loads(p.stdout.splitlines()[-1])
        self.assertNotEqual(p.returncode, 0, p.stdout)
        self.assertIn('Sony plain verification refused:', p.stderr)

    def test_complete_projection_and_explicit_raw_key_limit(self):
        report = self.cli(True)
        self.assertEqual(report['rows'], 10)
        self.assertEqual(report['tables'], 6)
        self.assertEqual(report['raw_key_projection'][0]['raw_keys'], ['276', '276.1'])

    def test_changed_rust_facts_fail(self):
        for old, new in [('"On"', '"Wrong"'), ('0u32', '1u32'), ('mask: 16383', 'mask: 1023'),
                         ('count: 3', 'count: 2'), ('Vc::ExpTime(6.0_f64', 'Vc::ExpTime(7.0_f64'),
                         ('Some(3)', 'Some(4)'), ('Fmt::U16, tags: T0', 'Fmt::U8, tags: T0'),
                         ('low_priority: true', 'low_priority: false'), ('raw: Raw::Store(Dm::FaceInfoOffset)', 'raw: Raw::None'),
                         ('^DSLR-(A450|A500|A550)$', '^DSLR-(A450|A500)$'), ('CAMERASETTINGS: usize = 0', 'CAMERASETTINGS: usize = 1')]:
            with self.subTest(change=new):
                self.rust.write_text(rust_fixture().replace(old, new, 1)); self.cli()

    def test_invalid_rust_numeric_types_and_enum_namespace_fail(self):
        for old, new in [('count: 3', 'count: 3.0_f64'), ('Fmt::U16', 'Abc::U16'),
                         ('Some(3)', 'Some(3.0_f64)'),
                         ('6.0_f64', '6u32'), ('CAMERASETTINGS: usize = 0', 'CAMERASETTINGS: usize = 0.0_f64')]:
            self.rust.write_text(rust_fixture().replace(old, new, 1)); self.cli()

    def test_full_file_and_literal_refusals(self):
        row = RUST_ROWS[4]
        for text in [rust_fixture()+ 'fn ignored() {}', rust_fixture()+'/* unterminated',
                     rust_fixture()+'#[rustfmt::skip]', rust_fixture().replace(row, '/*'+row+'*/'),
                     rust_fixture().replace(row, json.dumps(row)), rust_fixture().replace(row, row+row)]:
            self.rust.write_text(text); self.cli()

    def test_changed_live_semantics_cannot_be_copied_as_pass(self):
        for old, new in [("'276.1'", "'276.2'"), ("Mask=>16383", "Mask=>1023"),
                         ('PrintInt=>1', 'PrintInt=>2'), ('FIRST_ENTRY=>0', 'FIRST_ENTRY=>1'),
                         ('WRITABLE=>1', "HOOK=>'die'"), ("Name=>'Comp'", "Name=>'Comp', Hook=>'die'"),
                         ('6 - $val/8', '7 - $val/8'), ('DATAMEMBER=>[2]', 'DATAMEMBER=>[3]'),
                         ('IS_SUBDIR=>[72]', 'IS_SUBDIR=>[73]')]:
            with self.subTest(change=new):
                self.pm.write_text(perl_fixture().replace(old, new, 1)); self.cli()

    def test_raw_variant_order_and_new_fractional_collisions_fail(self):
        self.pm.write_text(perl_fixture().replace("0 => {Name=>'OtherFace'", "'0.1' => {Name=>'OtherFace'")); self.cli()
        self.pm.write_text(perl_fixture())
        rows = RUST_ROWS[2].splitlines()
        self.rust.write_text(rust_fixture().replace('\n'.join(rows), '\n'.join([rows[1], rows[0], rows[2]]))); self.cli()

    def test_empty_or_unmodeled_native_values_fail(self):
        for old, new in [("0=>'Off'", '0=>[]'), ("Name=>'Comp'", 'Name=>undef'),
                         (PERL_ROWS[4], "0 => {},"), (PERL_ROWS[4], '')]:
            self.pm.write_text(perl_fixture().replace(old, new)); self.cli()

    def test_native_other_closure_semantics_are_checked(self):
        self.cli(True)
        self.pm.write_text(perl_fixture().replace('sub {shift}', 'sub {shift() + 1}')); self.cli()

    def test_rounding_other_closure_and_changed_constant(self):
        body = 'sub {my ($val, $inv) = @_; return int($val + 0.5) unless $inv; return Image::ExifTool::IsFloat($val) ? $val : undef;}'
        self.pm.write_text(perl_fixture().replace('sub {shift}', body))
        self.rust.write_text(rust_fixture().replace('Other::Identity', 'Other::RoundHalfUp'))
        self.cli(True)
        self.pm.write_text(self.pm.read_text().replace('0.5', '0.6')); self.cli()

    def test_source_pin_loaded_identity_and_environment(self):
        self.env['PERL5OPT'] = '-MShouldNotLoad'
        self.env['PERL5LIB'] = str(self.base/'ambient')
        self.cli(True)
        self.pm.write_text(perl_fixture()+'$INC{"Image/ExifTool/Sony.pm"}="elsewhere";\n'); self.cli()
        self.pm.write_text(perl_fixture())
        self.core.write_text(self.core.read_text().replace(PIN, '0')); self.cli()

    def test_literal_whitespace_is_preserved(self):
        self.pm.write_text(perl_fixture().replace("0=>'Off'", "0=>'Off  // literal'"))
        self.rust.write_text(rust_fixture().replace('"Off"', '"Off  // literal"'))
        self.cli(True)
        self.rust.write_text(self.rust.read_text().replace('Off  //', 'Off //')); self.cli()

    def test_native_octal_and_changed_operators_are_not_decimal_aliases(self):
        for old, new in [('128) / 24', '0128) / 24'), ('0x20', '032'),
                         ('6 - $val/8', '6 + $val/8')]:
            self.pm.write_text(perl_fixture().replace(old, new)); self.cli()

    def test_quoted_numeric_text_and_missing_selected_source(self):
        self.pm.write_text(perl_fixture().replace("0=>'Off'", "0=>'010'"))
        self.rust.write_text(rust_fixture().replace('"Off"', '"010"'))
        self.cli(True)
        self.pm.unlink()
        self.cli()


if __name__ == '__main__':
    unittest.main()
