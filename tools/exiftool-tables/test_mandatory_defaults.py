"""Facts and closed admission for WriteExif %mandatory defaults."""
from __future__ import annotations
from copy import deepcopy
import json, os, sys
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
            with self.assertRaisesRegex(MandatoryRefused, 'executable new-directory flow'):
                compile_mandatory(capture(copied))

    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_general_writer_join_rejects_mixed_writeexif_identity(self):
        assert NATIVE is not None
        fact = capture(NATIVE[1])
        effective = {"__name": fact["writer"]["actual_name"], "source_file": fact["writer"]["source_file"],
                     "source_sha256": fact["writer"]["source_sha256"]}
        document = {"native_write_tables": {"Exif": {"Main": {"effective_write_proc": {"effective": effective}}}}}
        compile_mandatory_joined(fact, document)
        effective["source_sha256"] = '0' * 64
        with self.assertRaisesRegex(MandatoryRefused, 'do not join'):
            compile_mandatory_joined(fact, document)

if __name__ == '__main__': unittest.main()

class MandatoryCodegenTests(unittest.TestCase):
    @unittest.skipUnless(NATIVE is not None, 'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB must select a native source')
    def test_fresh_three_release_facts_render_and_rust_match_python(self):
        """Fresh canonical/11.78/12.64 captures drive actual rendered Rust."""
        from mandatory_defaults_codegen import evaluate, generate
        runtime = ROOT / 'src/writers/mandatory_defaults_runtime.rs'
        roots = [NATIVE[1]]
        evidence = Path(Path('/tmp/oxidex-sony-plain-current.txt').read_text().strip()) / 'shared-pilot/write-upgrade-integration-20260913/first-random-pair-20260913/source-attempt-01/sources'
        roots += [next(evidence.glob(f'exiftool-{version}-*/lib')) for version in ('11.78', '12.64')]
        for library in roots:
            with self.subTest(library=library):
                fact = capture(library)
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
 let values=defaults_for_new_directory(&writers::generated::MANDATORY_DEFAULTS,"IFD0",false,0,Some(JfifValues{{x:72,y:72,resolution_unit:1}})).unwrap();
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
        document = {"native_write_tables": {"Exif": {"Main": {"effective_write_proc": {"effective": effective}}}}}
        source, report = generate(fact, document)
        self.assertTrue(report['writer_tables_joined'])
        self.assertIn(fact['writer']['source_sha256'], source)
        effective["source_sha256"] = '0' * 64
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

    def test_codegen_refuses_bad_join_and_does_not_register_artifact(self):
        from mandatory_defaults_codegen import generate
        fact = {"schema": 1, "kind": "oxidex_exif_mandatory_defaults_fact"}
        with self.assertRaises(Exception): generate(fact)
        self.assertNotIn('mandatory-default', (ROOT / 'tools/exiftool-tables/artifacts.py').read_text())
