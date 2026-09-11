"""CLI contracts against tiny actual Perl tables, independent of any cache.

The synthetic tables are orchestration controls, not metadata-parity evidence.
The separately recorded pinned-source run compares all real Main/UID facts.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import verify_dicom_dict as verifier

HERE = Path(__file__).resolve().parent
PERL = shutil.which('/usr/bin/perl') or shutil.which('perl')
PIN = (HERE.parents[1] / '.exiftool-version').read_text().strip()
TABLE = """package Image::ExifTool::DICOM;
use strict;
use warnings;
our (%Main, %uid);
%Image::ExifTool::DICOM::Main = (
    GROUPS => { 0 => 'DICOM' },
    VARS => {},
    NOTES => 'Fixture table',
    '0010,0010' => { VR => 'PN', Name => 'FirstName' },
    '0010,0010' => { VR => 'PN', Name => 'PatientName' },
    '0028,0103' => { VR => 'US', Name => 'PixelRepresentation', PrintConv => { 0 => 'Unsigned', 1 => 'Signed' } },
    '0043,106f' => { VR => 'SL', Name => 'ScannerTableEntry' },
    '0074,100a' => { VR => 'CS', Name => 'ContactDisplayName' },
    '0074,100c' => { VR => 'CS', Name => 'ContactURI' },
    '7Fxx,0010' => { VR => 'OB', Name => 'VariablePixelData', Binary => 1 },
    'FFFE,E000' => 'Item',
);
%uid = (
    '1.2.3' => 'Original',
    '1.2.3' => 'Replacement ',
    '1.2.4' => 'Name "quoted"',
);
1;
"""


@unittest.skipUnless(PERL, 'a Perl interpreter with JSON::PP is required')
class DicomProducerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / 'selected'
        self.core, self.pm = self.make_source(self.source)
        self.out = self.root / 'dictionary.rs'
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith('PERL') and k != 'EXIFTOOL_PERL'}

    def make_source(self, root, version=PIN, table=TABLE):
        core = root / 'lib/Image/ExifTool.pm'
        pm = root / 'lib/Image/ExifTool/DICOM.pm'
        pm.parent.mkdir(parents=True)
        core.write_text(f"package Image::ExifTool; our $VERSION = '{version}'; 1;\n")
        pm.write_text(table)
        return core, pm

    def run_cli(self, *args, tool='gen_dicom_dict.py', env=None, explicit_perl=True):
        command = [sys.executable, str(HERE / tool), '--exiftool-dir', str(self.source)]
        if explicit_perl:
            command += ['--perl', PERL]
        command += ['--input' if tool.startswith('verify_') else '--out', str(self.out)]
        return subprocess.run(command + list(args), env=self.env | (env or {}),
                              text=True, capture_output=True)

    def generate(self):
        result = self.run_cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        return self.out.read_text()

    def refuse_preserving_output(self, expected, *args, **kwargs):
        self.out.write_bytes(b'existing output\x00must survive\n')
        self.out.chmod(0o640)
        before = (self.out.read_bytes(), self.out.stat().st_mode, self.out.stat().st_mtime_ns)
        result = self.run_cli(*args, **kwargs)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(expected, result.stderr)
        self.assertEqual((self.out.read_bytes(), self.out.stat().st_mode,
                          self.out.stat().st_mtime_ns), before)

    def test_deterministic_output_and_native_complete_fact_comparison(self):
        first = self.generate()
        self.assertEqual(self.generate(), first)
        facts = verifier.rust_facts(first)
        self.assertEqual(len(facts['main']), 7)
        self.assertEqual(len(facts['uid']), 2)
        self.assertEqual(facts['main']['0010,0010'], ['PatientName', 'PN', 0, 0])
        self.assertEqual(facts['main']['0028,0103'], ['PixelRepresentation', 'US', 0, 1])
        self.assertEqual(facts['main']['7Fxx,0010'], ['VariablePixelData', 'OB', 1, 0])
        self.assertEqual(facts['main']['FFFE,E000'], ['Item', None, 0, 0])
        self.assertEqual(facts['uid']['1.2.3'], 'Replacement ')
        for key in ('0043,106f', '0074,100a', '0074,100c'):
            self.assertIn(key, facts['main'])
        report = self.root / 'report.json'
        result = self.run_cli('--json-out', str(report), tool='verify_dicom_dict.py')
        self.assertEqual(result.returncode, 0, result.stderr)
        doc = json.loads(report.read_text())
        self.assertEqual((doc['main_rows'], doc['uid_rows']), (7, 2))
        self.assertEqual(doc['identity']['source'], str(self.source.resolve()))

    def test_check_compares_entire_file_without_writing(self):
        self.generate()
        before = (self.out.read_bytes(), self.out.stat().st_mtime_ns)
        self.assertEqual(self.run_cli('--check').returncode, 0)
        self.assertEqual((self.out.read_bytes(), self.out.stat().st_mtime_ns), before)
        self.out.write_text(self.out.read_text().replace('.ok()', '.err()', 1))
        changed = self.out.read_bytes()
        result = self.run_cli('--check')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('output differs', result.stderr)
        self.assertEqual(self.out.read_bytes(), changed)
        self.out.unlink()
        self.assertNotEqual(self.run_cli('--check').returncode, 0)
        self.assertFalse(self.out.exists())

    def test_selected_version_beats_optional_parent_stamp(self):
        (self.root / '.exiftool-version').write_text('99.99')
        self.generate()
        self.core.write_text("package Image::ExifTool; our $VERSION = '99.99'; 1;\n")
        (self.root / '.exiftool-version').write_text(PIN)
        self.refuse_preserving_output('source version')
        (self.root / '.exiftool-version').unlink()
        self.refuse_preserving_output('source version')

    def test_interpreter_override_and_hostile_perl_environment(self):
        foreign = self.root / 'foreign'
        self.make_source(foreign, table=TABLE.replace('PatientName', 'ForeignName'))
        env = {'PERL5OPT': '-MImage::ExifTool::DICOM', 'PERL5LIB': str(foreign / 'lib'),
               'PERLLIB': str(foreign / 'lib'), 'EXIFTOOL_PERL': '/nonexistent/interpreter'}
        result = self.run_cli(env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('PatientName', self.out.read_text())
        self.assertNotIn('ForeignName', self.out.read_text())
        result = self.run_cli('--check', env={'EXIFTOOL_PERL': PERL}, explicit_perl=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.refuse_preserving_output('no usable explicit Perl',
                                     env={'EXIFTOOL_PERL': '/nonexistent/interpreter'}, explicit_perl=False)

    def test_incomplete_selected_tree_cannot_fall_back_to_ambient_modules(self):
        foreign = self.root / 'foreign'
        self.make_source(foreign)
        for path in (self.core, self.pm):
            original = path.read_bytes()
            path.unlink()
            self.refuse_preserving_output('required selected source is missing',
                                         env={'PERL5LIB': str(foreign / 'lib')})
            path.write_bytes(original)

    def test_loaded_module_identity_must_match_selected_source(self):
        foreign = self.root / 'foreign'
        _, other_pm = self.make_source(foreign)
        self.pm.write_text(TABLE + f"$INC{{'Image/ExifTool/DICOM.pm'}} = '{other_pm}';\n")
        self.refuse_preserving_output('do not belong to the selected source')

    def test_malformed_and_unmodeled_sources_fail_before_output_replacement(self):
        controls = [
            (TABLE + 'not valid perl !!!', 'Perl DICOM oracle failed'),
            (TABLE.replace("Name => 'PatientName'", "Name => 'PatientName', NewField => 1"), 'unmodeled Main field'),
            (TABLE.replace("'0010,0010' => { VR => 'PN', Name => 'PatientName' }",
                           '"0010,0010" => { VR => \'PN\', Name => \'PatientName\' }'), 'DICOM fact mismatches'),
            (TABLE.replace("'0010,0010' => { VR => 'PN', Name => 'PatientName' }",
                           "'invalid-key' => { VR => 'PN', Name => 'PatientName' }"), 'unmodeled Main key'),
            (TABLE.replace("1 => 'Signed'", "1 => 'Other'"), 'unmodeled PrintConv'),
            (TABLE.replace('Binary => 1', 'Binary => 2'), 'unmodeled Binary'),
            (TABLE.replace("'Replacement '", "'0'"), 'invalid/falsy UID'),
            (TABLE.replace("'1.2.3' => 'Replacement '", "'1.2.3' => ('Replacement ' . 'suffix')"), 'unmodeled %uid entry'),
            (TABLE.replace("'0010,0010' => { VR => 'PN', Name => 'PatientName' }",
                           "'0010,0010' => { Name => 'PatientName', VR => 'PN' }"), 'unmodeled Main entry'),
        ]
        for table, reason in controls:
            with self.subTest(reason=reason):
                self.pm.write_text(table)
                self.refuse_preserving_output(reason)

    def test_empty_source_tables_are_not_success(self):
        self.pm.write_text("package Image::ExifTool::DICOM; our (%Main, %uid); 1;\n")
        self.refuse_preserving_output('empty DICOM')

    def test_output_preserves_mode_and_refuses_symlink(self):
        self.out.write_text('prior')
        self.out.chmod(0o640)
        self.generate()
        self.assertEqual(self.out.stat().st_mode & 0o777, 0o640)
        target = self.root / 'target.rs'
        target.write_text('must survive')
        self.out.unlink()
        self.out.symlink_to(target)
        result = self.run_cli()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('not a regular file', result.stderr)
        self.assertEqual(target.read_text(), 'must survive')

    def test_independent_verifier_rejects_each_fact_mutation_and_omission(self):
        text = self.generate()
        mutations = [
            ('name', text.replace('e("PatientName",', 'e("WrongName",')),
            ('vr', text.replace('e("PatientName", b"PN")', 'e("PatientName", b"LO")')),
            ('binary', text.replace('eb("VariablePixelData",', 'e("VariablePixelData",')),
            ('printconv', text.replace('ep("PixelRepresentation",', 'e("PixelRepresentation",')),
            ('uid', text.replace('"Replacement "', '"Replacement"')),
            ('key-case', text.replace('"0043,106f"', '"0043,106F"')),
            ('wildcard', text.replace('"7Fxx,0010"', '"7F00,0010"')),
            ('missing', text.replace('    ("0010,0010", e("PatientName", b"PN")),\n', '')),
            ('extra', text.replace('    ("0010,0010",', '    ("0000,0000", e("Extra", b"PN")),\n    ("0010,0010",')),
            ('constructor-body', text.replace('binary: true,', 'binary: false,')),
            ('duplicate', text.replace('    ("1.2.3", "Replacement "),',
                                       '    ("1.2.3", "Replacement "),\n    ("1.2.3", "Replacement "),')),
            ('comment-only', '/*\n' + text + '\n*/'),
            ('string-only', 'const FORGERY: &str = r###"' + text + '"###;'),
        ]
        for name, bad in mutations:
            with self.subTest(name=name):
                self.assertNotEqual(text, bad, 'mutation must reach a real declaration')
                self.out.write_text(bad)
                result = self.run_cli(tool='verify_dicom_dict.py')
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn('DICOM verification refused', result.stderr)
                self.assertEqual(self.out.read_text(), bad)


if __name__ == '__main__':
    unittest.main()
