"""Complete historical source profiles; portable facts and optional native proof.

The fixtures are captured from the preserved11.78/12.64 native source pair.
Native checks require OXIDEX_RAW_JFIF_VERSION_LIBS to map those releases to
explicit source lib directories, plus EXIFTOOL_PERL. Run under validation.lock.
"""
from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest

from raw_jfif_codegen import Refused, compile_fact, evaluate, generate, source_contract

HERE = Path(__file__).resolve().parent
VERSIONS = ('11.78', '12.64', '13.59')


def fixture(version):
    return json.loads((HERE / 'testdata' / ('raw_jfif_' + version.replace('.', '_') + '_fact.json')).read_text())


def raw_cases(recipe):
    payload = b'JFIF\0\x01\x02\x00\x00\x00\x00\x00\0\0'
    cases = [(recipe.marker, payload[:length]) for length in range(len(payload) + 1)]
    cases += [(recipe.marker, b'JFIF\0\x01\x02'+bytes([unit])+b'\x01\x00\x00\x90\0\0') for unit in (0, 1, 2, 3)]
    cases += [(0xe2, payload), (recipe.marker, b'NOPE\0'+payload[5:])]
    return cases


class HistoricalRawJfifPortableTests(unittest.TestCase):
    def test_captured_versions_compile_same_unsigned_raw_and_creation_operands(self):
        recipes = [compile_fact(fixture(version)) for version in VERSIONS]
        self.assertEqual(recipes[0], recipes[1])
        self.assertEqual(recipes[1], recipes[2])
        self.assertEqual(recipes[0].creation_skip_markers, (224,))
        self.assertEqual(recipes[0].creation_wait_for_directories, ('IFD0', 'ExtendedEXIF'))
        for recipe in recipes:
            self.assertEqual(evaluate(recipe, recipe.marker, b'JFIF\0\x01\x02\x02\x01\x2c'),
                             {'JFIFResolutionUnit': 2, 'JFIFXResolution': 300})
            self.assertEqual(evaluate(recipe, recipe.marker, b'JFIF\0\x01\x02'+bytes(7)),
                             {'JFIFResolutionUnit': 0, 'JFIFXResolution': 0, 'JFIFYResolution': 0})

    def test_every_body_insertion_and_source_join_refuses_for_every_version(self):
        for version in VERSIONS:
            fact = fixture(version)
            for name in fact['functions']:
                with self.subTest(version=version, function=name):
                    changed = deepcopy(fact)
                    changed['functions'][name]['body'] += '\n($val += 1);'
                    with self.assertRaises(Refused):
                        compile_fact(changed)
                    changed = deepcopy(fact)
                    changed['functions'][name]['source']['sha256'] = '0' * 64
                    with self.assertRaises(Refused):
                        compile_fact(changed)

    def test_profile_mixing_is_not_an_independent_function_allowlist(self):
        current = fixture('13.59')
        for version in VERSIONS[:2]:
            changed = fixture(version)
            self.assertNotEqual(changed['functions']['WriteJPEG']['body'], current['functions']['WriteJPEG']['body'])
            changed['functions']['WriteJPEG']['body'] = current['functions']['WriteJPEG']['body']
            with self.assertRaises(Refused):
                compile_fact(changed)

    def test_historical_field_operands_propagate_and_unknown_controls_refuse(self):
        for version in VERSIONS[:2]:
            with self.subTest(version=version):
                fact = fixture(version)
                changed = deepcopy(fact)
                changed['table']['entries']['3']['RawConv'] = '$$self{NativeRawX} = $val'
                self.assertEqual(compile_fact(changed).fields[1].property, 'NativeRawX')
                changed['table']['entries']['3']['RawConv'] += '; $val += 1;'
                with self.assertRaises(Refused):
                    compile_fact(changed)
                changed = deepcopy(fact)
                changed['marker_names']['entries']['224'] = 'APP2'
                with self.assertRaises(Refused):
                    compile_fact(changed)
                changed = deepcopy(fact)
                changed['exif_header']['hex'] = '587869660000'
                with self.assertRaises(Refused):
                    compile_fact(changed)
                changed = deepcopy(fact)
                changed['exif_header']['source']['sha256'] = '0' * 64
                with self.assertRaises(Refused):
                    compile_fact(changed)

    def test_canonical13_59_emitter_and_report_are_byte_identical(self):
        source, report = generate(fixture('13.59'))
        self.assertEqual(hashlib.sha256(source.encode()).hexdigest(),
                         '0e084e1867a04b6150bb994e96332c594ed2ab73278133096b120e668f6d83e4')
        self.assertEqual(hashlib.sha256(json.dumps(report, sort_keys=True).encode()).hexdigest(),
                         'e21022d33979a9f59cc87696bdfa6a3357cc8c50a912d17cecdcb58610dbc5d3')
        self.assertEqual(source_contract(fixture('13.59'))[0].name, 'raw_jfif_source_grammar.json')


@unittest.skipUnless(os.environ.get('EXIFTOOL_PERL') and os.environ.get('OXIDEX_RAW_JFIF_VERSION_LIBS'),
                     'selected historical native libraries and interpreter are required')
class HistoricalRawJfifNativeTests(unittest.TestCase):
    def test_actual_historical_writers_preserve_every_raw_boundary(self):
        from test_raw_jfif_codegen import capture, native
        libraries = json.loads(os.environ['OXIDEX_RAW_JFIF_VERSION_LIBS'])
        self.assertEqual(set(libraries), set(VERSIONS[:2]))
        for version, library in libraries.items():
            with self.subTest(version=version):
                fact = capture(library)
                self.assertEqual(fact['native_identity']['exiftool_version'], version)
                recipe = compile_fact(fact)
                cases = raw_cases(recipe)
                self.assertEqual(native(library, recipe, cases), [evaluate(recipe, *case) or {} for case in cases])

    def test_copied_historical_byte_order_operand_matches_native(self):
        from test_raw_jfif_codegen import capture, native
        for version, library in json.loads(os.environ['OXIDEX_RAW_JFIF_VERSION_LIBS']).items():
            with self.subTest(version=version), tempfile.TemporaryDirectory() as temporary:
                copied = Path(temporary) / 'lib'
                shutil.copytree(library, copied)
                source = copied / 'Image/ExifTool/Writer.pl'
                before = "last unless $$editDirs{JFIF};\n                    SetByteOrder('MM');"
                body = source.read_text()
                self.assertEqual(body.count(before), 1)
                source.write_text(body.replace(before, before.replace("'MM'", "'II'")))
                recipe = compile_fact(capture(copied))
                self.assertEqual(recipe.byte_order, 'II')
                cases = [(recipe.marker, b'JFIF\0\x01\x02\x01\x02\x03\x04\x05\x06\x07')]
                self.assertEqual(native(copied, recipe, cases), [evaluate(recipe, *case) for case in cases])

    def test_actual_copied_dispatch_statement_is_refused(self):
        from test_raw_jfif_codegen import capture
        for version, library in json.loads(os.environ['OXIDEX_RAW_JFIF_VERSION_LIBS']).items():
            with self.subTest(version=version), tempfile.TemporaryDirectory() as temporary:
                copied = Path(temporary) / 'lib'
                shutil.copytree(library, copied)
                source = copied / 'Image/ExifTool/Writer.pl'
                before = 'last unless $$editDirs{JFIF};'
                body = source.read_text()
                self.assertEqual(body.count(before), 1)
                source.write_text(body.replace(before, before+' $$segDataPt = substr($$segDataPt,1);'))
                with self.assertRaises(Refused):
                    compile_fact(capture(copied))


if __name__ == '__main__':
    unittest.main()
