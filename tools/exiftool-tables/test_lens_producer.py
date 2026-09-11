"""CLI/source/shape controls for the lens producer, using tiny Perl modules.

Synthetic controls test refusal and preservation. Genuine pinned complete-table
validation is a separate invocation of verify_lens_alternatives.py.
"""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
PERL = os.environ.get('LENS_TEST_PERL') or shutil.which('perl')
RUSTFMT = shutil.which('rustfmt')


@unittest.skipUnless(PERL and RUSTFMT, 'Perl and rustfmt required for producer controls')
class LensProducerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='lens-producer-')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / 'repo'
        self.tools = self.root / 'tools/exiftool-tables'
        self.tools.mkdir(parents=True)
        for name in ('dump_lens_alternatives.pl', 'verify_lens_alternatives.py'):
            if (HERE / name).exists():
                shutil.copy2(HERE / name, self.tools / name)
        (self.root / '.exiftool-version').write_text('13.59\n')
        shutil.copy2(HERE.parents[1] / 'rustfmt.toml', self.root / 'rustfmt.toml')
        self.source = self.base / 'source'
        self.lib = self.source / 'lib/Image/ExifTool'
        self.lib.mkdir(parents=True)
        self.core = self.lib.parent / 'ExifTool.pm'
        self.core.write_text("package Image::ExifTool; our $VERSION = '13.59'; 1;\n")
        self.canon("1=>'Base Canon', '1.1'=>'Alt Canon', 2=>'Solo Canon'")
        self.pentax("'3 1'=>'Base Pentax', '3 1.1'=>'Alt Pentax', OTHER=>sub {undef}")
        self.olympus("'0 01 00'=>'Olympus Lens'")
        (self.lib / 'Panasonic.pm').write_text('package Image::ExifTool::Panasonic; 1;\n')
        self.out = self.base / 'output.rs'
        self.out.write_bytes(b'existing output\n')
        self.env = {k:v for k,v in os.environ.items() if k not in ('PERL5OPT','PERL5LIB','PERLLIB')}

    def canon(self, values):
        (self.lib / 'Canon.pm').write_text('package Image::ExifTool::Canon; our %canonLensTypes = (' + values + '); our %FileInfo = (61=>{PrintConv=>{0=>"n/a",257=>"Canon RF 50mm F1.2L USM"}}); 1;\n')

    def pentax(self, values):
        (self.lib / 'Pentax.pm').write_text('package Image::ExifTool::Pentax; our %pentaxLensTypes = (' + values + '); 1;\n')

    def olympus(self, values):
        (self.lib / 'Olympus.pm').write_text('package Image::ExifTool::Olympus; our %Equipment = (0x0201=>{PrintConv=>{' + values + '}}); 1;\n')

    def run_producer(self, *args, perl_args=(), env=None):
        return subprocess.run([PERL,*perl_args,str(self.tools / 'dump_lens_alternatives.pl'),
            '--exiftool-dir',str(self.source),'--rustfmt',RUSTFMT,*map(str,args)],
            env={**self.env,**(env or {})},capture_output=True,text=True)

    def refuses(self, message, **kwargs):
        before = self.out.read_bytes()
        result = self.run_producer('--out',self.out,**kwargs)
        self.assertNotEqual(result.returncode,0,result.stdout)
        self.assertIn(message,result.stderr)
        self.assertEqual(result.stdout,'')
        self.assertEqual(self.out.read_bytes(),before)
        return result

    def test_rf_table_growth_and_malformed_values_preserve_output(self):
        for values in ("{}", "{257=>'RF', '257.1'=>'Alternate'}", "{257=>0}"):
            with self.subTest(values=values):
                self.canon("1=>'Base Canon', '1.1'=>'Alt Canon'")
                with (self.lib / 'Canon.pm').open('a') as handle:
                    handle.write("$FileInfo{61}{PrintConv}="+values+"; 1;\n")
                self.refuses('')

    def test_default_body_and_complete_stdout_contracts(self):
        body = self.run_producer()
        self.assertEqual(body.returncode,0,body.stderr)
        self.assertTrue(body.stdout.startswith('///'))
        self.assertNotIn('//!',body.stdout)
        full = self.run_producer('--full-file')
        self.assertEqual(full.returncode,0,full.stderr)
        self.assertTrue(full.stdout.startswith('//! The ambiguous half'))
        self.assertIn('Why this is a separate table',full.stdout)
        self.assertIn('`.exiftool-version`, 13.59',full.stdout)
        self.assertIn('CANON_LENS_ALTERNATIVES: [(i64, &str, &[&str]); 2]',full.stdout)
        self.assertIn('PENTAX_LENS_ALTERNATIVES: [(&str, &[&str]); 1]',full.stdout)

    def test_deterministic_complete_output_mode_and_check(self):
        self.out.chmod(0o640)
        first = self.run_producer('--out',self.out)
        self.assertEqual(first.returncode,0,first.stderr)
        expected = self.out.read_bytes()
        self.assertTrue(expected.startswith(b'//!'))
        self.assertEqual(self.out.stat().st_mode & 0o777,0o640)
        again = self.run_producer('--out',self.out)
        self.assertEqual(again.returncode,0,again.stderr)
        self.assertEqual(self.out.read_bytes(),expected)
        checked = self.run_producer('--check',self.out)
        self.assertEqual(checked.returncode,0,checked.stderr)
        self.assertEqual(self.out.read_bytes(),expected)
        self.out.write_bytes(expected.replace(b'Alt Canon',b'Altered Canon'))
        stale = self.out.read_bytes()
        checked = self.run_producer('--check',self.out)
        self.assertNotEqual(checked.returncode,0)
        self.assertIn('differ',checked.stderr)
        self.assertEqual(self.out.read_bytes(),stale)

    def test_wrong_selected_version_preserves_output(self):
        self.core.write_text("package Image::ExifTool; our $VERSION = '13.58'; 1;\n")
        self.refuses('disagrees with repository pin')

    def test_missing_selected_module_does_not_fall_back_to_inc(self):
        foreign = self.base / 'foreign'
        shutil.copytree(self.source,foreign)
        (self.lib / 'Canon.pm').unlink()
        self.refuses('selected source missing Image/ExifTool/Canon.pm',perl_args=('-I'+str(foreign/'lib'),))

    def test_preloaded_same_version_foreign_module_is_refused(self):
        foreign = self.base / 'foreign'
        shutil.copytree(self.source,foreign)
        self.refuses('loaded outside the selected source',perl_args=('-I'+str(foreign/'lib'),'-MImage::ExifTool::Canon'))

    def test_ambient_perl_injection_is_refused(self):
        for name,value in [('PERL5OPT','-Mstrict'),('PERL5LIB',str(self.source/'lib')),('PERLLIB',str(self.source/'lib'))]:
            with self.subTest(name=name):self.refuses('refusing ambient '+name,env={name:value})

    def test_false_nonstring_and_reference_alternatives_are_refused(self):
        for value in ["''","'0'",'undef','0','7','[]','{}','sub {1}']:
            with self.subTest(value=value):
                self.canon("1=>'Base Canon','1.1'=>"+value)
                self.refuses('expected a truthy string scalar')

    def test_nonstring_or_false_base_is_refused(self):
        for value in ["''","'0'",'undef','42','[]','sub {1}']:
            with self.subTest(value=value):
                self.canon('1=>'+value+",'1.1'=>'Alt'")
                self.refuses('expected a truthy string scalar')

    def test_gapped_or_orphan_alternatives_refuse_instead_of_truncating(self):
        self.canon("1=>'Base','1.2'=>'Alt'")
        self.refuses('noncontiguous alternatives')
        self.canon("2=>'Other','1.1'=>'Alt'")
        self.refuses('orphan alternatives')

    def test_unsupported_keys_and_changed_directive_refuse(self):
        for key in ['1.0','1.01','1.word','NEW_DIRECTIVE']:
            with self.subTest(key=key):
                self.canon("1=>'Base','1.1'=>'Alt','"+key+"'=>'Unexpected'")
                self.refuses('unsupported key')
        self.canon("1=>'Base','1.1'=>'Alt'")
        self.pentax("'3 1'=>'Pentax','3 1.1'=>'Alt',OTHER=>'New semantics'")
        self.refuses('OTHER: expected CODE directive')

    def test_canon_raw_keys_preserve_shared_labels_and_distinct_alternatives(self):
        self.canon("129=>'Canon EF 300mm f/2.8L USM',136=>'Canon EF 300mm f/2.8L USM','136.1'=>'Tamron SP 15-30mm f/2.8 Di VC USD (A012)'")
        result=self.run_producer('--out',self.out)
        self.assertEqual(result.returncode,0,result.stderr)
        checked=self.run_verifier()
        self.assertEqual(checked.returncode,0,checked.stdout+checked.stderr)
        self.assertIn('129',self.out.read_text())
        self.assertIn('136',self.out.read_text())
        self.assertEqual(self.out.read_text().count('"Canon EF 300mm f/2.8L USM"'),2)

    def test_canon_raw_id_spelling_and_range_refuse(self):
        for key in ['01', '-0', '1 2']:
            self.canon("1=>'Base','1.1'=>'Alt','"+key+"'=>'Other'")
            self.refuses('unsupported key')
        self.canon("1=>'Base','1.1'=>'Alt','9223372036854775808'=>'Other'")
        self.refuses('outside i64 range')

    def test_pentax_all_ambiguous_label_collisions_refuse(self):
        for tail in ["'3 2'=>'Shared'", "'3 2'=>'Shared','3 2.1'=>'Other'"]:
            self.pentax("'3 1'=>'Shared','3 1.1'=>'Alt',"+tail)
            self.refuses('shared by multiple base IDs')

    def test_known_notes_directives_are_validated_and_excluded(self):
        self.pentax("'3 1'=>'Base','3 1.1'=>'Alt',OTHER=>sub {undef},Notes=>'Documentation'")
        self.olympus("'0 01 00'=>'Olympus Lens',Notes=>'Documentation'")
        result=self.run_producer('--out',self.out)
        self.assertEqual(result.returncode,0,result.stderr)
        checked=self.run_verifier()
        self.assertEqual(checked.returncode,0,checked.stdout+checked.stderr)
        self.olympus("'0 01 00'=>'Olympus Lens',Notes=>[]")
        self.refuses('expected a truthy string scalar')

    def test_unambiguous_duplicate_labels_remain_allowed(self):
        self.canon("1=>'Base','1.1'=>'Alt',2=>'Same',3=>'Same'")
        result = self.run_producer('--out',self.out)
        self.assertEqual(result.returncode,0,result.stderr)

    def test_empty_or_vacuous_tables_refuse(self):
        for value in ['',"1=>'Only base'"]:
            self.canon(value);self.refuses('empty ')
        self.canon("1=>'Base','1.1'=>'Alt'")
        self.pentax('OTHER=>sub {undef}');self.refuses('empty base')
        self.pentax("'3 1'=>'Base','3 1.1'=>'Alt',OTHER=>sub {undef}")
        self.olympus('');self.refuses('empty base')

    def test_olympus_fractional_growth_refuses(self):
        self.olympus("'0 01 00'=>'Olympus','0 01 00.1'=>'New alternative'")
        self.refuses('grew fractional keys')

    def test_formatter_failure_and_bad_output_options_preserve_output(self):
        before = self.out.read_bytes()
        result = self.run_producer('--out',self.out,'--rustfmt','/usr/bin/false')
        self.assertNotEqual(result.returncode,0)
        self.assertIn('rustfmt failed',result.stderr)
        self.assertEqual(self.out.read_bytes(),before)
        result = self.run_producer('--out',self.out,'--check',self.out)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('mutually exclusive',result.stderr)
        self.assertEqual(self.out.read_bytes(),before)

    def test_output_symlink_refuses_without_touching_target(self):
        link = self.base/'link.rs';link.symlink_to(self.out)
        result = self.run_producer('--out',link)
        self.assertNotEqual(result.returncode,0)
        self.assertTrue(link.is_symlink())
        self.assertEqual(self.out.read_bytes(),b'existing output\n')

    def test_strings_are_escaped_without_losing_literal_content(self):
        self.canon(r'''1=>'Base', '1.1'=>"quote \" slash \\ tab\t newline\n null\0", '1.2'=>'Second' ''')
        result = self.run_producer('--out',self.out)
        self.assertEqual(result.returncode,0,result.stderr)
        text = self.out.read_text()
        self.assertIn(r'quote \" slash \\',text)
        self.assertIn(r'\u{9}',text)
        self.assertIn(r'\u{a}',text)
        self.assertIn(r'\u{0}',text)
        self.assertLess(text.index('quote'),text.index('Second'))

    def run_verifier(self):
        return subprocess.run([sys.executable,str(self.tools/'verify_lens_alternatives.py'),str(self.out),
            '--exiftool-dir',str(self.source),'--perl',PERL],env=self.env,capture_output=True,text=True)

    def test_independent_complete_fact_oracle_and_mutation_controls(self):
        self.canon("1=>'Base Canon','1.1'=>'First','1.2'=>'Second',2=>'Solo'")
        result=self.run_producer('--out',self.out)
        self.assertEqual(result.returncode,0,result.stderr)
        valid=self.out.read_text()
        checked=self.run_verifier()
        self.assertEqual(checked.returncode,0,checked.stdout+checked.stderr)
        mutations=[valid.replace('First','Changed'), valid.replace('Base Canon','Changed Base'),
                   valid.replace('["First", "Second"]','["Second", "First"]'),
                   valid+'pub fn extra() {}\n',valid.replace('; 2] = [','; 3] = [',1)]
        for changed in mutations:
            with self.subTest(change=changed[-100:]):
                self.assertNotEqual(changed,valid)
                self.out.write_text(changed)
                checked=self.run_verifier()
                self.assertNotEqual(checked.returncode,0,checked.stdout)
        self.out.write_text(valid)
        self.pentax("'3 1'=>'Base Pentax','3 1.1'=>'Alt Pentax','3 2'=>'Base Pentax',OTHER=>sub {undef}")
        checked=self.run_verifier()
        self.assertNotEqual(checked.returncode,0)
        self.assertIn('ambiguous label',checked.stdout)
        self.assertIn('"projection_matches": true',checked.stdout)

    def test_independent_oracle_preserves_escaped_literal_sequences(self):
        self.canon(r'''1=>'Base', '1.1'=>'literal \u{41} and \n', '1.2'=>"tab\tquote \"" ''')
        result=self.run_producer('--out',self.out)
        self.assertEqual(result.returncode,0,result.stderr)
        checked=self.run_verifier()
        self.assertEqual(checked.returncode,0,checked.stdout+checked.stderr)


if __name__ == '__main__':
    unittest.main()
