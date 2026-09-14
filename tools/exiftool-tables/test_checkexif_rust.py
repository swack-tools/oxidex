"""Compare generated Rust CheckExif execution with actual native helper results.

Freshly capture each copied native source. No canonical result is carried across
the source-change cases. This tests direct helpers, not public input sanitation.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import checkexif_native_differential as native
import checkexif_rust_codegen as generator
from scalar_helper_codegen import rust_string

ROOT = Path(__file__).resolve().parents[2]


def _bytes(raw):
    return '[' + ','.join(map(str, bytes.fromhex(raw))) + ']'


def _property(value):
    if value is None or isinstance(value, dict) and value['kind'] == 'undefined':
        return 'PropertyValue::Undefined'
    if isinstance(value, int):
        return 'PropertyValue::Integer(' + str(value) + ')'
    if isinstance(value, str):
        return 'PropertyValue::Text(' + rust_string(value) + ')'
    if value['kind'] == 'integer':
        return _property(value['value'])
    if value['kind'] == 'bytes':
        return 'PropertyValue::Bytes(&' + _bytes(value['hex']) + ')'
    if value['kind'] == 'utf8':
        return _property(value['text'])
    raise ValueError('unrepresented fixture property')


def _scalar(value):
    if value.get('kind') == 'undefined' or value.get('defined') is False:
        return 'Scalar::Undefined'
    if value.get('kind') == 'bytes' or value.get('utf8') is False:
        return 'Scalar::Bytes(vec!' + _bytes(value['hex']) + ')'
    text = value['text'] if 'kind' in value else bytes.fromhex(value['hex']).decode('utf8')
    return 'Scalar::Utf8(' + rust_string(text) + '.to_owned())'


@unittest.skipUnless(os.environ.get('EXIFTOOL_PERL') and os.environ.get('OXIDEX_PINNED_EXIFTOOL'),
                     'requires explicit native interpreter and selected source')
class NativeCheckExifRustTests(unittest.TestCase):
    def test_native_source_changes_reach_the_real_rust_executor(self):
        perl = native.resolve_perl(Path(os.environ['EXIFTOOL_PERL']))
        library = native.resolve_library(Path(os.environ['OXIDEX_PINNED_EXIFTOOL']))
        cases = json.loads((Path(__file__).parent / 'testdata/checkexif_native_cases.json').read_text())
        for name, group in [('undefined_group', None), ('numeric_group', 1)]:
            cases.append({'name': name, 'group0': group, 'scalar': {'kind': 'bytes', 'hex': '61'}})
        env = {key: value for key, value in os.environ.items() if key not in {'PERL5LIB', 'PERLLIB', 'PERL5OPT'}}
        results_by_variant = {}
        with tempfile.TemporaryDirectory(prefix='oxidex-checkexif-rust-') as directory:
            root = Path(directory)
            for variant in ('canonical', 'selector_order', 'numeric_group', 'empty_group'):
                with self.subTest(variant=variant):
                    work = root / variant
                    work.mkdir()
                    selected = library
                    if variant != 'canonical':
                        selected = work / 'lib'
                        shutil.copytree(library, selected)
                        source = selected / native.SOURCE_RELATIVE
                        text = source.read_text()
                        if variant == 'selector_order':
                            for old, new in native.MUTATION:
                                self.assertEqual(text.count(old), 1)
                                text = text.replace(old, new)
                        else:
                            start = text.index('sub CheckExif($$$)\n{')
                            end = text.index('\n#------------------------------------------------------------------------------', start)
                            body = text[start:end]
                            self.assertEqual(body.count("'MakerNotes'"), 1)
                            literal = "'1'" if variant == 'numeric_group' else "''"
                            text = text[:start] + body.replace("'MakerNotes'", literal) + text[end:]
                        source.write_text(text)
                    capture = subprocess.run(
                        [str(perl), str(ROOT / 'tools/exiftool-tables/dump_tables.pl'), str(selected), 'Exif'],
                        env=env, capture_output=True, timeout=60, check=True)
                    document = json.loads(capture.stdout)
                    rust, report = generator.generate(document)
                    recipe = next(row for row in report['recipes'] if
                                  ['Exif', 'Main', 'Image::ExifTool::Exif::Main'] in row['source_tables']
                                  or ('Exif', 'Main', 'Image::ExifTool::Exif::Main') in row['source_tables'])
                    observed = native.run_native(perl, selected, cases)
                    self.assertEqual(observed['source_sha256'], recipe['check_proc']['source_sha256'])
                    self.assertEqual(observed['body_sha256'], recipe['check_proc']['body_sha256'])
                    self.assertEqual(observed['check_value']['source_sha256'], recipe['check_value']['source_sha256'])
                    self.assertEqual(observed['check_value']['body_sha256'], recipe['check_value']['body_sha256'])
                    self.assertEqual(observed['source_sha256'], hashlib.sha256((selected / native.SOURCE_RELATIVE).read_bytes()).hexdigest())
                    self.assertEqual(len(observed['results']), len(cases))
                    self.assertEqual([row['name'] for row in observed['results']], [row['name'] for row in cases])
                    results_by_variant[variant] = observed['results']
                    rules = work / 'rules.rs'
                    rules.write_text(rust)
                    harness = [
                        '#![allow(dead_code)]',
                        '#[path=' + rust_string(str(ROOT / 'src/error/mod.rs')) + '] mod error;',
                        'mod writers {',
                        '#[path=' + rust_string(str(ROOT / 'src/writers/generated_scalar.rs')) + '] pub(crate) mod generated_scalar;',
                        '#[path=' + rust_string(str(ROOT / 'src/writers/generated_scalar_rules.rs')) + '] pub(crate) mod generated_scalar_rules;',
                        '#[path=' + rust_string(str(ROOT / 'src/writers/generated_checkexif.rs')) + '] pub(crate) mod generated_checkexif; }',
                        '#[path=' + rust_string(str(rules)) + '] mod rules;',
                        'use writers::generated_checkexif::*; use writers::generated_scalar::Scalar;',
                        '#[test] fn native_cases() {',
                        'let recipe = rules::CHECK_EXIF_RECIPES.iter().find(|r| r.source_tables.iter().any(|t| t.full_name == "Image::ExifTool::Exif::Main")).unwrap();',
                    ]
                    for case, result in zip(cases, observed['results'], strict=True):
                        props = ','.join('Property { name: ' + rust_string(prop) + ', value: ' + _property(case.get(key)) + ' }'
                                         for key, prop in [('format', 'Format'), ('writable', 'Writable'), ('count', 'Count')])
                        harness += [
                            'let input = CheckExifInput { tag_properties: &[' + props + '],',
                            'table_properties: &[Property { name: "WRITABLE", value: ' + _property(case.get('table_writable')) + '}],',
                            'tag_groups: &[Property { name: "0", value: ' + _property(case.get('group0')) + '}] };',
                            'let observed = check_exif(recipe, &input, ' + _scalar(case['scalar']) + ').unwrap();',
                            'let (value,error) = match observed { CheckExifResult::Bypassed(value) => (value,None), CheckExifResult::Rejected {value,error} => (value,Some(error)), CheckExifResult::Checked(result) => (result.value,result.error) };',
                            'assert_eq!(value, ' + _scalar(result) + ', ' + rust_string(case['name'] + ' value') + ');',
                            'assert_eq!(error, ' + ('None' if result['error'] is None else 'Some(' + rust_string(result['error']) + ')') + ', ' + rust_string(case['name'] + ' error') + ');',
                        ]
                    harness.append('}')
                    program = work / 'proof.rs'
                    program.write_text('\n'.join(harness))
                    compiled = subprocess.run(['rustc', '--edition=2024', '--test', str(program), '-o', str(work / 'proof')],
                                              capture_output=True, text=True, timeout=60)
                    self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
                    executed = subprocess.run([str(work / 'proof'), '--nocapture'], capture_output=True, text=True, timeout=30)
                    self.assertEqual(executed.returncode, 0, executed.stdout + executed.stderr)
        for variant, results in results_by_variant.items():
            if variant != 'canonical':
                self.assertNotEqual(results_by_variant['canonical'], results, variant + ' mutation had no observable effect')


if __name__ == '__main__':
    unittest.main()
