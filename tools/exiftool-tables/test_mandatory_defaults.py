"""Facts and closed admission for WriteExif %mandatory defaults."""
from __future__ import annotations
from copy import deepcopy
from functools import lru_cache
import hashlib, json, os, sys
from pathlib import Path
import shutil, subprocess, tempfile, unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from mandatory_defaults import MandatoryRefused, compile_mandatory, compile_mandatory_joined

ROOT = HERE.parents[1]
CAPTURE = ROOT / 'tools/exiftool-tables/capture_exif_mandatory_fact.pl'
def selected_native() -> tuple[Path, Path] | None:
    raw_perl, raw_lib = os.environ.get('EXIFTOOL_PERL'), os.environ.get('OXIDEX_EXIFTOOL_LIB')
    if not raw_perl or not raw_lib:
        return None
    resolved_perl = Path(shutil.which(raw_perl) or raw_perl).expanduser().resolve()
    lib = Path(raw_lib).expanduser().resolve()
    lib = (lib / 'lib').resolve() if (lib / 'lib').is_dir() else lib
    return (resolved_perl, lib) if resolved_perl.is_file() and (lib / 'Image/ExifTool/WriteExif.pl').is_file() else None

NATIVE = selected_native()

def capture(lib: Path) -> dict:
    if NATIVE is None: raise RuntimeError('selected native Perl/library is unavailable')
    env = {k:v for k,v in os.environ.items() if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
    return json.loads(subprocess.run([str(NATIVE[0]), str(CAPTURE), str(lib)], env=env,
                                     check=True, capture_output=True, text=True).stdout)

class MandatoryTests(unittest.TestCase):
    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_actual_capture_and_copied_source_value_removal_mutations_propagate(self):
        assert NATIVE is not None
        canonical = compile_mandatory(capture(NATIVE[1]))
        current = {d.directory: {v.tag_id: v.value for v in d.defaults} for d in canonical.directories}
        self.assertEqual(current['IFD0'][531], 1)
        self.assertEqual(canonical.selection.disabled_when, 'noMandatory')
        self.assertEqual(canonical.selection.new_directory_when, 'numEntries == 0')
        self.assertEqual(canonical.jfif_override.assignments, ((282, 'JFIFXResolution', 0),
                                                                (283, 'JFIFYResolution', 0),
                                                                (296, 'JFIFResolutionUnit', 1)))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); copied = root/'lib'; shutil.copytree(NATIVE[1], copied)
            source = copied/'Image/ExifTool/WriteExif.pl'; target = source
            body = source.read_text()
            self.assertEqual(body.count('0x0213 => 1'), 1)
            target.write_text(body.replace('0x0213 => 1', '0x0213 => 9'))
            changed = compile_mandatory(capture(copied))
            values = {d.directory: {v.tag_id: v.value for v in d.defaults} for d in changed.directories}
            self.assertEqual(values['IFD0'][531], 9)
            target.write_text(body.replace("0x0213 => 1,        # YCbCrPositioning (centered)\n", ''))
            removed = compile_mandatory(capture(copied))
            values = {d.directory: {v.tag_id: v.value for v in d.defaults} for d in removed.directories}
            self.assertNotIn(531, values['IFD0'])

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_rejects_source_control_changes_and_unrepresentable_operands(self):
        assert NATIVE is not None
        fact = capture(NATIVE[1])
        changed = deepcopy(fact)
        changed['new_directory_context_deparse'] = changed['new_directory_context_deparse'].replace('unless ($numEntries)', 'if ($numEntries)')
        with self.assertRaisesRegex(MandatoryRefused, 'closed grammar'):
            compile_mandatory(changed)
        changed = deepcopy(fact); changed['lexical']['entries']['IFD0']['not-an-id'] = 1
        with self.assertRaisesRegex(MandatoryRefused, 'unrepresentable'):
            compile_mandatory(changed)

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_cleanup_source_requires_all_mandatory_shrink_no_next_and_ifd1_omission(self):
        assert NATIVE is not None
        fact = capture(NATIVE[1])
        cleanup = compile_mandatory(fact).cleanup
        self.assertEqual((cleanup.all_mandatory, cleanup.no_next_ifd,
                          cleanup.entry_count_shrinks_or_new, cleanup.omit_empty_ifd1),
                         (True, True, True, True))
        for original, replacement in (
            ('$allMandatory and not $isNextIFD', '$allMandatory or not $isNextIFD'),
            ('$newEntries < $numEntries', '$newEntries <= $numEntries'),
            ('if ($ifd and not $newEntries)', 'if ($ifd or not $newEntries)'),
        ):
            changed = deepcopy(fact)
            changed['mandatory_cleanup_source'] = changed['mandatory_cleanup_source'].replace(original, replacement)
            changed['mandatory_cleanup_source_sha256'] = hashlib.sha256(
                changed['mandatory_cleanup_source'].encode()).hexdigest()
            with self.subTest(original=original), self.assertRaisesRegex(MandatoryRefused, 'closed grammar'):
                compile_mandatory(changed)

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_executable_flow_mutation_refuses_but_dead_source_text_does_not(self):
        with tempfile.TemporaryDirectory() as temporary:
            assert NATIVE is not None
            copied = Path(temporary) / 'lib'; shutil.copytree(NATIVE[1], copied)
            target = copied / 'Image/ExifTool/WriteExif.pl'
            body = target.read_text()
            # This is a valid-looking copy in comments.  It must not affect the
            # executable CV deparse that the compiler admits.
            target.write_text(body + '\n# $mandatory = $mandatory{$dirName} unless $noMandatory;\n')
            compile_mandatory(capture(copied))
            # Change the actual statement executed by WriteExif.  Fresh capture
            # changes the B::Deparse fragment and closed admission refuses.
            target.write_text(body.replace('$mandatory = $mandatory{$dirName} unless $noMandatory;',
                                           '$mandatory = $mandatory{$dirName};'))
            with self.assertRaisesRegex(MandatoryRefused, '(executable new-directory flow|executable body review hash)'):
                compile_mandatory(capture(copied))

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_general_writer_join_rejects_mixed_writeexif_identity(self):
        assert NATIVE is not None
        fact = capture(NATIVE[1])
        effective = {"__name": fact["writer"]["actual_name"], "source_file": fact["writer"]["source_file"],
                     "source_sha256": fact["writer"]["source_sha256"]}
        document = MandatoryNumericEncodingTests()._document(fact)
        compile_mandatory_joined(fact, document)
        document["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["source_sha256"] = '0' * 64
        with self.assertRaisesRegex(MandatoryRefused, 'do not join'):
            compile_mandatory_joined(fact, document)
        document["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["source_sha256"] = fact["writer"]["source_sha256"]
        document["native_write_helpers"]["write_value"]["requested_binding"] = "Image::ExifTool::BorrowedWriteValue"
        with self.assertRaisesRegex(MandatoryRefused, 'WriteValue helper'):
            compile_mandatory_joined(fact, document)


class MandatoryCodegenTests(unittest.TestCase):
    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_fresh_three_release_facts_render_and_rust_match_python(self):
        """Only the reviewed executable WriteExif body is emitted; older bodies refuse."""
        from mandatory_defaults_codegen import evaluate, generate
        runtime = ROOT / 'src/writers/mandatory_defaults_runtime.rs'
        roots = [NATIVE[1]]
        evidence = Path(Path('/tmp/oxidex-sony-plain-current.txt').read_text().strip()) / 'shared-pilot/write-upgrade-integration-20260913/first-random-pair-20260913/source-attempt-01/sources'
        roots += [next(evidence.glob(f'exiftool-{version}-*/lib')) for version in ('11.78', '12.64')]
        for library in roots:
            with self.subTest(library=library):
                fact = capture(library)
                if library != NATIVE[1]:
                    with self.assertRaisesRegex(MandatoryRefused, 'executable body review hash'):
                        generate(fact)
                    continue
                rendered, report = generate(fact)
                self.assertFalse(report['writer_tables_joined'])
                recipe = compile_mandatory(fact)
                expected = evaluate(recipe, 'IFD0', False, 0, (72, 72, 1))
                self.assertTrue(expected)
                with tempfile.TemporaryDirectory() as temporary:
                    temporary = Path(temporary); generated = temporary / 'generated.rs'; generated.write_text(rendered)
                    driver, binary = temporary / 'driver.rs', temporary / 'driver'
                    driver.write_text(f'''mod writers {{
#[path = "{runtime}"] pub mod mandatory_defaults_runtime;
#[path = "{generated}"] pub mod generated;
}}
use writers::mandatory_defaults_runtime::*;
fn main() {{
 let values=defaults_for_new_directory(&writers::generated::MANDATORY_DEFAULTS,"IFD0",false,0,Some(JfifValues{{x:Some(72),y:Some(72),resolution_unit:Some(1)}})).unwrap();
 for item in values {{ match item.value {{ MandatoryValue::Integer(value)=>println!("{{}}=i:{{}}",item.tag_id,value), MandatoryValue::Text(value)=>println!("{{}}=s:{{}}",item.tag_id,value) }} }}
}}''')
                    subprocess.run(['rustc', '--edition=2021', str(driver), '-o', str(binary)], check=True, capture_output=True, text=True)
                    actual = tuple(sorted((int(key), int(value[2:]) if value.startswith('i:') else value[2:])
                                          for key, value in (line.split('=', 1) for line in subprocess.run([str(binary)], check=True, capture_output=True, text=True).stdout.splitlines())))
                self.assertEqual(actual, expected)

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_codegen_requires_same_selected_writeexif_when_general_sidecar_is_supplied(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        fact = capture(NATIVE[1])
        effective = {"__name": fact["writer"]["actual_name"], "source_file": fact["writer"]["source_file"],
                     "source_sha256": fact["writer"]["source_sha256"]}
        document = MandatoryNumericEncodingTests()._document(fact)
        source, report = generate(fact, document)
        self.assertTrue(report['writer_tables_joined'])
        self.assertIn(fact['writer']['source_sha256'], source)
        self.assertNotIn(str(NATIVE[0]), json.dumps(report))
        with self.assertRaisesRegex(MandatoryRefused, 'Perl differs'):
            generate(fact, document, '/nonexistent/perl')
        document["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["source_sha256"] = '0' * 64
        with self.assertRaisesRegex(MandatoryRefused, 'do not join'):
            generate(fact, document)
        for missing in ('Image/ExifTool.pm', 'Image/ExifTool/Writer.pl', 'Image/ExifTool/Exif.pm'):
            broken = deepcopy(fact); broken['loaded_exiftool_closure'].pop(missing)
            with self.subTest(missing=missing), self.assertRaisesRegex(Exception, 'closure'):
                generate(broken)

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_copied_native_default_mutation_reaches_rendered_rust_operands(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        with tempfile.TemporaryDirectory() as temporary:
            copied = Path(temporary) / 'lib'; shutil.copytree(NATIVE[1], copied)
            source = copied / 'Image/ExifTool/WriteExif.pl'; body = source.read_text()
            self.assertEqual(body.count('0x0213 => 1'), 1)
            source.write_text(body.replace('0x0213 => 1', '0x0213 => 9'))
            rendered, report = generate(capture(copied))
            self.assertIn('tag_id: 0x0213, value: MandatoryValue::Integer(9)', rendered)
            self.assertNotEqual(report['recipe']['recipe_sha256'], generate(capture(NATIVE[1]))[1]['recipe']['recipe_sha256'])

    def test_codegen_refuses_bad_join_and_registration_is_declared(self):
        from mandatory_defaults_codegen import generate
        fact = {"schema": 1, "kind": "oxidex_exif_mandatory_defaults_fact"}
        with self.assertRaises(Exception): generate(fact)
        self.assertIn('mandatory-default-rules', (ROOT / 'tools/exiftool-tables/artifacts.py').read_text())

class MandatoryNumericEncodingTests(unittest.TestCase):
    """Native direct WriteValue packing proof for source-derived IFD0 defaults."""
    @staticmethod
    @lru_cache(maxsize=8)
    def _native_document(library: str, closure: tuple) -> dict:
        # Fresh selected-native Exif-only capture, cached only for the exact
        # library/loaded-source closure. No synthetic substring-only helper.
        assert NATIVE is not None
        env = {k:v for k,v in os.environ.items() if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
        result = subprocess.run([str(NATIVE[0]), str(HERE / 'dump_tables.pl'), library, 'Exif'],
                                env=env, check=True, capture_output=True, text=True)
        document = json.loads(result.stdout)
        return {key:value for key,value in document.items() if key.startswith('native_write_') or key == 'exiftool_version'}

    def _document(self, fact: dict, library: Path | None = None) -> dict:
        assert NATIVE is not None
        return deepcopy(self._native_document(str(library or NATIVE[1]),
                        tuple(sorted(fact['loaded_exiftool_closure'].items()))))

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_ifd1_generated_defaults_match_native_bytes_in_both_orders(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        fact = capture(NATIVE[1])
        rendered, report = generate(fact, self._document(fact), str(NATIVE[0]))
        directory = next(row for row in report['recipe']['directories'] if row['directory'] == 'IFD1')
        encodings = {row['tag_id']: row for row in report['recipe']['encodings']}
        operands = [{**encodings[row['tag_id']], 'value': row['value']}
                    for row in directory['defaults']]
        self.assertTrue(operands, 'native directory must exercise actual defaults')
        env = {key: value for key, value in os.environ.items()
               if key not in {'PERL5LIB', 'PERLLIB', 'PERL5OPT'}}
        program = r"""
require Image::ExifTool; require q(Image/ExifTool/Writer.pl); require JSON::PP;
my $rows = JSON::PP::decode_json(do { local $/; <STDIN> });
for my $order ('MM', 'II') {
    Image::ExifTool::SetByteOrder($order);
    for my $row (@$rows) {
        my $bytes = Image::ExifTool::WriteValue($row->{value}, $row->{format_name}, 1);
        die 'native default was not encoded' unless defined $bytes;
        printf "%s:%04x:%d=%s\n", $order, $row->{tag_id}, $row->{tiff_type}, unpack('H*', $bytes);
    }
}
"""
        expected = subprocess.run([str(NATIVE[0]), '-I' + str(NATIVE[1]), '-e', program],
                                  input=json.dumps(operands), env=env, check=True,
                                  capture_output=True, text=True).stdout.splitlines()
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            generated = temporary / 'generated.rs'; generated.write_text(rendered)
            runtime = ROOT / 'src/writers/mandatory_defaults_runtime.rs'
            driver = temporary / 'driver.rs'; binary = temporary / 'driver'
            driver.write_text(f"""mod writers {{
#[path = "{runtime}"] pub mod mandatory_defaults_runtime;
#[path = "{generated}"] pub mod generated;
}}
use writers::mandatory_defaults_runtime::*;
fn main() {{
 let recipe=&writers::generated::MANDATORY_DEFAULTS;
 let defaults=defaults_for_new_directory(recipe,"IFD1",false,0,None).unwrap();
 for (label,order) in [("MM",TiffByteOrder::Big),("II",TiffByteOrder::Little)] {{
  for value in encode_mandatory_defaults(recipe,&defaults,order).unwrap() {{
   print!("{{}}:{{:04x}}:{{}}=",label,value.tag_id,value.tiff_type);
   for b in value.bytes {{ print!("{{:02x}}",b); }} println!();
  }}
 }}
}}""")
            subprocess.run(['rustc', '--edition=2021', str(driver), '-o', str(binary)],
                           check=True, capture_output=True, text=True)
            actual = subprocess.run([str(binary)], check=True, capture_output=True, text=True).stdout.splitlines()
        self.assertEqual(len(actual), 2 * len(operands))
        self.assertEqual(sorted(actual), sorted(expected))

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_actual_writevalue_bytes_match_generated_ifd0_encoder_and_type_mutation_propagates(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        canonical_fact = capture(NATIVE[1])
        rendered, report = generate(canonical_fact, self._document(canonical_fact), str(NATIVE[0]))
        self.assertIn('tag_id: 0x0213, format_name: "int16u"', rendered)
        # IFD1 is selected by the same captured `$tagTablePtr->{id}` / WriteValue
        # path. These are source-captured defaults, not a handwritten IFD1 list.
        self.assertIn('tag_id: 0x0103, format_name: "int16u"', rendered)
        self.assertIn('tag_id: 0x011a, format_name: "rational64u"', rendered)
        self.assertIn('tag_id: 0x011b, format_name: "rational64u"', rendered)
        self.assertIn('tag_id: 0x0128, format_name: "int16u"', rendered)
        # The same `%mandatory` map also has an integer ExifIFD value. Its
        # raw Exif/Main row lacks a direct WriteGroup, so it is retained as an
        # explicit omission instead of causing a false IFD1 rejection or a
        # guessed encoder.
        self.assertEqual(report['recipe']['unencoded_numeric_defaults'], ({
            'directory': 'ExifIFD', 'tag_id': 40961,
            'reason': 'direct_write_group_unrepresented'},))
        # This calls the real selected Writer.pl helper.  WriteExif's proven
        # new-directory branch invokes this helper directly for these values.
        env = {key:value for key,value in os.environ.items() if key not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
        native = subprocess.run([str(NATIVE[0]), '-I'+str(NATIVE[1]), '-e',
            "require Image::ExifTool; require q(Image/ExifTool/Writer.pl); for my $f (qw(int16u rational64u)) { my $v=Image::ExifTool::WriteValue(72,$f,1); print qq($f=),unpack('H*',$v),qq(\\n); }"],
            env=env, check=True, capture_output=True, text=True).stdout
        self.assertEqual(native.splitlines(), ['int16u=0048', 'rational64u=0000004800000001'])
        with tempfile.TemporaryDirectory() as temporary:
            temporary = Path(temporary); generated = temporary/'generated.rs'; generated.write_text(rendered)
            runtime = ROOT/'src/writers/mandatory_defaults_runtime.rs'; driver=temporary/'driver.rs'; binary=temporary/'driver'
            driver.write_text(f'''mod writers {{
#[path = "{runtime}"] pub mod mandatory_defaults_runtime;
#[path = "{generated}"] pub mod generated;
}}
use writers::mandatory_defaults_runtime::*;
fn main() {{
 let defaults=defaults_for_new_directory(&writers::generated::MANDATORY_DEFAULTS,"IFD0",false,0,Some(JfifValues{{x:Some(72),y:Some(72),resolution_unit:Some(1)}})).unwrap();
 for order in [TiffByteOrder::Big,TiffByteOrder::Little] {{ for value in encode_ifd0_defaults(&writers::generated::MANDATORY_DEFAULTS,&defaults,order).unwrap() {{ print!("{{:04x}}:{{}}=",value.tag_id,value.tiff_type); for b in value.bytes {{ print!("{{:02x}}",b); }} println!(); }} }}
}}''')
            subprocess.run(['rustc','--edition=2021',str(driver),'-o',str(binary)],check=True,capture_output=True,text=True)
            actual=subprocess.run([str(binary)],check=True,capture_output=True,text=True).stdout.splitlines()
        self.assertIn('0213:3=0001', actual)
        self.assertIn('011a:5=0000004800000001', actual)
        self.assertIn('011b:5=0000004800000001', actual)

        # Change the selected native Exif row, freshly capture it, and prove
        # both the emitted format and the real Writer.pl bytes change.
        with tempfile.TemporaryDirectory() as temporary:
            copied = Path(temporary)/'lib'; shutil.copytree(NATIVE[1], copied)
            exif = copied/'Image/ExifTool/Exif.pm'; body=exif.read_text()
            anchor="0x213 => {\n        Name => 'YCbCrPositioning',\n        Protected => 1,\n        Writable => 'int16u',"
            self.assertEqual(body.count(anchor), 1)
            exif.write_text(body.replace(anchor, anchor.replace("'int16u'", "'rational64u'")))
            changed_fact=capture(copied); changed,_=generate(changed_fact,self._document(changed_fact, copied),str(NATIVE[0]))
            self.assertIn('tag_id: 0x0213, format_name: "rational64u"',changed)
            generated = Path(temporary)/'changed.rs'; generated.write_text(changed)
            runtime = ROOT/'src/writers/mandatory_defaults_runtime.rs'; driver=Path(temporary)/'changed-driver.rs'; binary=Path(temporary)/'changed-driver'
            driver.write_text(f'''mod writers {{ #[path = "{runtime}"] pub mod mandatory_defaults_runtime; #[path = "{generated}"] pub mod generated; }}
use writers::mandatory_defaults_runtime::*;
fn main() {{ let d=defaults_for_new_directory(&writers::generated::MANDATORY_DEFAULTS,"IFD0",false,0,None).unwrap(); for v in encode_ifd0_defaults(&writers::generated::MANDATORY_DEFAULTS,&d,TiffByteOrder::Big).unwrap() {{ if v.tag_id==0x0213 {{ for b in v.bytes {{ print!("{{:02x}}",b); }} }} }} }}''')
            subprocess.run(['rustc','--edition=2021',str(driver),'-o',str(binary)],check=True,capture_output=True,text=True)
            self.assertEqual(subprocess.run([str(binary)],check=True,capture_output=True,text=True).stdout, '0000000100000001')
            changed_native=subprocess.run([str(NATIVE[0]), '-I'+str(copied), '-e',
                "require Image::ExifTool; require q(Image/ExifTool/Writer.pl); print unpack('H*',Image::ExifTool::WriteValue(1,'rational64u',1));"],env=env,check=True,capture_output=True,text=True).stdout
            self.assertEqual(changed_native, '0000000100000001')

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_generated_minimal_ifd0_tiff_is_sorted_and_preserves_byte_order(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        fact = capture(NATIVE[1]); rendered, _ = generate(fact, self._document(fact), str(NATIVE[0]))
        with tempfile.TemporaryDirectory() as temporary:
            temporary=Path(temporary); generated=temporary/'generated.rs'; generated.write_text(rendered)
            runtime=ROOT/'src/writers/mandatory_defaults_runtime.rs'; driver=temporary/'driver.rs'; binary=temporary/'driver'
            driver.write_text(f'''mod writers {{ #[path = "{runtime}"] pub mod mandatory_defaults_runtime; #[path = "{generated}"] pub mod generated; }}
use writers::mandatory_defaults_runtime::*;
fn main() {{ for order in [TiffByteOrder::Big,TiffByteOrder::Little] {{ let t=minimal_ifd0_tiff(&writers::generated::MANDATORY_DEFAULTS,order,false,0,Some(JfifValues{{x:Some(72),y:Some(72),resolution_unit:Some(1)}})).unwrap(); for b in t {{ print!("{{:02x}}",b); }} println!(); }} }}''')
            subprocess.run(['rustc','--edition=2021',str(driver),'-o',str(binary)],check=True,capture_output=True,text=True)
            big,little=subprocess.run([str(binary)],check=True,capture_output=True,text=True).stdout.splitlines()
        self.assertTrue(big.startswith('4d4d002a000000080004'))
        self.assertIn('011a0005000000010000003e', big)
        self.assertIn('011b00050000000100000046', big)
        self.assertIn('01280003000000010002000002130003000000010001000000000000', big)
        self.assertTrue(little.startswith('49492a00080000000400'))
        self.assertIn('1a010500010000003e000000', little)
        self.assertIn('28010300010000000200000013020300010000000100000000000000', little)

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_jfif_y_defined_gate_preserves_absence_and_refuses_partial_operands(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        fact = capture(NATIVE[1]); rendered, _ = generate(fact, self._document(fact), str(NATIVE[0]))
        with tempfile.TemporaryDirectory() as temporary:
            temporary=Path(temporary); generated=temporary/'generated.rs'; generated.write_text(rendered)
            runtime=ROOT/'src/writers/mandatory_defaults_runtime.rs'; driver=temporary/'driver.rs'; binary=temporary/'driver'
            driver.write_text(f'''mod writers {{ #[path = "{runtime}"] pub mod mandatory_defaults_runtime; #[path = "{generated}"] pub mod generated; }}
use writers::mandatory_defaults_runtime::*;
fn main() {{ let r=&writers::generated::MANDATORY_DEFAULTS; println!("{{}}", defaults_for_new_directory(r,"IFD0",false,0,Some(JfifValues{{x:Some(72),y:None,resolution_unit:Some(1)}})).unwrap().len()); println!("{{}}", defaults_for_new_directory(r,"IFD0",false,0,Some(JfifValues{{x:None,y:Some(72),resolution_unit:Some(1)}})).is_err()); }}''')
            subprocess.run(['rustc','--edition=2021',str(driver),'-o',str(binary)],check=True,capture_output=True,text=True)
            self.assertEqual(subprocess.run([str(binary)],check=True,capture_output=True,text=True).stdout.splitlines(), ['1','true'])

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_copied_native_writevalue_dispatch_mutation_refuses_fresh_capture(self):
        """A changed executable numeric dispatch cannot reuse old Rust packing."""
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        dumper = ROOT / 'tools/exiftool-tables/dump_tables.pl'
        with tempfile.TemporaryDirectory() as temporary:
            copied = Path(temporary) / 'lib'; shutil.copytree(NATIVE[1], copied)
            writer = copied / 'Image/ExifTool/Writer.pl'; body = writer.read_text()
            anchor = '$packed .= &$proc($val);'
            self.assertEqual(body.count(anchor), 1)
            writer.write_text(body.replace(anchor, '$packed .= &$proc(0);'))
            env = {key:value for key,value in os.environ.items() if key not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
            changed_document = json.loads(subprocess.run([str(NATIVE[0]), str(dumper), str(copied), "Exif"], env=env, check=True, capture_output=True, text=True).stdout)
            self.assertIn('&$proc(0)', changed_document['native_write_helpers']['write_value']['__deparse'])
            with self.assertRaisesRegex(MandatoryRefused, 'numeric WriteValue dispatch'):
                generate(capture(copied), changed_document, str(NATIVE[0]))

    @unittest.skipUnless(NATIVE is not None, 'selected native source required')
    def test_copied_numeric_statement_insertion_changes_native_bytes_and_refuses(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        with tempfile.TemporaryDirectory() as temporary:
            copied = Path(temporary) / 'lib'; shutil.copytree(NATIVE[1], copied)
            writer = copied / 'Image/ExifTool/Writer.pl'
            body = writer.read_text(); anchor = '$packed .= &$proc($val);'
            self.assertEqual(body.count(anchor), 1)
            writer.write_text(body.replace(anchor, '$val += 1; ' + anchor))
            fact = capture(copied); document = self._document(fact, copied)
            deparse = document['native_write_helpers']['write_value']['__deparse']
            # All old required substring tokens survive this mutation.
            for token in ("$writeValueProc{$format}", "if ($proc)", "split(' ', $val, 0)", "($packed .= &$proc($val))"):
                self.assertIn(token, deparse)
            env = {k:v for k,v in os.environ.items() if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
            native = subprocess.run([str(NATIVE[0]), '-I'+str(copied), '-e',
                "require Image::ExifTool; require q(Image/ExifTool/Writer.pl); print unpack('H*',Image::ExifTool::WriteValue(72,'int16u',1));"],
                env=env, check=True, capture_output=True, text=True).stdout
            self.assertEqual(native, '0049')
            with self.assertRaisesRegex(MandatoryRefused, 'fully consumed grammar'):
                generate(fact, document, str(NATIVE[0]))

    @unittest.skipUnless(NATIVE is not None, 'selected native source required')
    def test_validation_and_packing_dependency_insertions_and_provenance_refuse(self):
        assert NATIVE is not None
        fact = capture(NATIVE[1]); original = self._document(fact)
        def functions(document):
            write = document['native_write_helpers']['write_value']
            dispatch = write['lexical_hashes']['bindings']['%writeValueProc']['entries']
            result = []
            def walk(helper):
                if helper.get('resolved') is not True: return
                result.append(helper)
                for dep in helper.get('dependencies', {}).values(): walk(dep)
            walk(write)
            for name in ('int8u', 'int8s', 'int16s', 'int16u', 'int32s', 'int32u', 'rational64u'): walk(dispatch[name])
            return result
        expected = {'WriteValue', 'IsInt', 'IsHex', 'IsFloat', 'IsRational',
                    'Set8u', 'Set8s', 'Set16s', 'Set16u', 'Set32s', 'Set32u', 'SetRational64u', 'Rationalize', 'AssembleRational', 'DoPackStd'}
        self.assertEqual({f['__name'].rsplit('::', 1)[1] for f in functions(original)}, expected)
        for index, function in enumerate(functions(original)):
            with self.subTest(helper=function['__name']):
                changed = deepcopy(original); helper = functions(changed)[index]
                helper['__deparse'] = helper['__deparse'].replace('use strict;', 'use strict; ($_[0] += 1);')
                with self.assertRaisesRegex(MandatoryRefused, 'fully consumed grammar'):
                    compile_mandatory_joined(fact, changed)
                changed = deepcopy(original); functions(changed)[index]['source_sha256'] = '0' * 64
                with self.assertRaisesRegex(MandatoryRefused, 'join'):
                    compile_mandatory_joined(fact, changed)
        for key in ('source', 'format_name', 'format_size'):
            changed = deepcopy(original)
            if key == 'source': changed['native_write_format_registry'][key]['sha256'] = '0' * 64
            else: changed['native_write_format_registry'][key][3] = 'int32u' if key == 'format_name' else 4
            with self.subTest(registry=key), self.assertRaises(MandatoryRefused):
                compile_mandatory_joined(fact, changed)
        for name in ('%unpackMotorola', '%unpackIntel'):
            changed = deepcopy(original)
            changed['native_write_helpers']['set_byte_order']['lexical_hashes']['bindings'][name]['entries']['S'] = 'C'
            with self.subTest(packing_map=name), self.assertRaisesRegex(MandatoryRefused, 'packing templates'):
                compile_mandatory_joined(fact, changed)

    @unittest.skipUnless(NATIVE is not None, 'selected native source required')
    def test_copied_native_packing_dependencies_and_format_registry_refuse(self):
        assert NATIVE is not None
        mutations = (
            ('Image/ExifTool.pm', "sub Set16u(@) { return DoPackStd('S', @_); }", "sub Set16u(@) { $_[0] += 1; return DoPackStd('S', @_); }"),
            ('Image/ExifTool.pm', "sub Set32u(@) { return DoPackStd('L', @_); }", "sub Set32u(@) { $_[0] += 1; return DoPackStd('L', @_); }"),
            ('Image/ExifTool.pm', 'my $val = pack($unpackStd{$_[0]}, $_[1]);', 'my $val = pack($unpackStd{$_[0]}, $_[1]); $val .= chr(0);'),
            ('Image/ExifTool/Writer.pl', "return (0, 1) if $val == 0;", "return (0, 1) if $val == 0; $val += 1;"),
            ('Image/ExifTool.pm', "my %unpackMotorola = ( S => 'n',", "my %unpackMotorola = ( S => 'v',"),
        )
        for relative, before, after in mutations:
            with self.subTest(mutation=before), tempfile.TemporaryDirectory() as temporary:
                copied = Path(temporary) / 'lib'; shutil.copytree(NATIVE[1], copied)
                target = copied / relative; body = target.read_text()
                self.assertEqual(body.count(before), 1); target.write_text(body.replace(before, after))
                fact = capture(copied); document = self._document(fact, copied)
                with self.assertRaises(MandatoryRefused): compile_mandatory_joined(fact, document)

    @unittest.skipUnless(NATIVE is not None, 'selected native source required')
    def test_native_integer_boundaries_match_rust_in_both_byte_orders(self):
        from mandatory_defaults_codegen import generate
        assert NATIVE is not None
        fact = capture(NATIVE[1]); rendered, _ = generate(fact, self._document(fact))
        env = {k:v for k,v in os.environ.items() if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
        native = subprocess.run([str(NATIVE[0]), '-I'+str(NATIVE[1]), '-e',
            "require Image::ExifTool; require q(Image/ExifTool/Writer.pl); for my $order (qw(MM II)) { Image::ExifTool::SetByteOrder($order); for my $fmt (qw(int16u rational64u)) { my @vals = $fmt eq 'int16u' ? (0,1,65535) : (0,1,65535,4294967295); for my $v (@vals) { print unpack('H*',Image::ExifTool::WriteValue($v,$fmt,1)),qq(\\n); } } }"],
            env=env, check=True, capture_output=True, text=True).stdout.splitlines()
        with tempfile.TemporaryDirectory() as temporary:
            temporary=Path(temporary); generated=temporary/'generated.rs'; generated.write_text(rendered)
            runtime=ROOT/'src/writers/mandatory_defaults_runtime.rs'; driver=temporary/'driver.rs'; binary=temporary/'driver'
            driver.write_text(f'''mod writers {{ #[path = "{runtime}"] pub mod mandatory_defaults_runtime; #[path = "{generated}"] pub mod generated; }}
use writers::mandatory_defaults_runtime::*;
fn main() {{ let r=&writers::generated::MANDATORY_DEFAULTS;
 for order in [TiffByteOrder::Big,TiffByteOrder::Little] {{
  for (tag,values) in [(531,&[0,1,65535][..]),(282,&[0,1,65535,4294967295][..])] {{ for value in values {{
   let input=[MandatoryDefault{{tag_id:tag,value:MandatoryValue::Integer(*value)}}];
   let encoded=encode_ifd0_defaults(r,&input,order).unwrap();
   assert_eq!(encoded[0].count,1); for b in &encoded[0].bytes {{ print!("{{:02x}}",b); }} println!();
  }} }}
 }}
 for (tag,value) in [(531,-1),(531,65536),(282,-1),(282,4294967296)] {{
  assert!(encode_ifd0_defaults(r,&[MandatoryDefault{{tag_id:tag,value:MandatoryValue::Integer(value)}}],TiffByteOrder::Big).is_err());
 }}
 assert!(encode_ifd0_defaults(r,&[MandatoryDefault{{tag_id:531,value:MandatoryValue::Text("1.5")}}],TiffByteOrder::Big).is_err());
}}''')
            subprocess.run(['rustc','--edition=2021',str(driver),'-o',str(binary)],check=True,capture_output=True,text=True)
            actual=subprocess.run([str(binary)],check=True,capture_output=True,text=True).stdout.splitlines()
        self.assertEqual(len(native),14)
        self.assertEqual(actual,native)

if __name__ == '__main__': unittest.main()
