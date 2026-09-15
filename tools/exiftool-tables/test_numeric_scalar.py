"""Portable source mutation and actual shared Rust numeric-runtime checks."""
import copy
import json
import re
from pathlib import Path
import subprocess
import tempfile
import unittest

from checkexif_recipes import RecipeRefused
from numeric_scalar import compile_numeric_scalar, render_numeric

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).with_name('testdata') / 'numeric_scalar_source.json'

def source():
    return json.loads(FIXTURE.read_text())

class NumericScalarTests(unittest.TestCase):
    def test_native_source_compiles_ordered_dispatch_and_bounds(self):
        recipe = compile_numeric_scalar(source())
        self.assertEqual(recipe.formats, ('int16u', 'rational64u'))
        self.assertEqual(recipe.maxima, (65535, 4294967295))

    def test_cleanup_capabilities_do_not_leak_into_public_fixed_two_format_recipe(self):
        recipe = compile_numeric_scalar(source())
        # The source capture now authenticates seven WriteValue forms for
        # private mandatory-directory cleanup. Public generated_scalar keeps
        # its two-slot ABI and original ordered operands.
        self.assertEqual(recipe.formats, ('int16u', 'rational64u'))
        self.assertEqual(recipe.maxima, (65535, 4294967295))
        rendered = render_numeric(recipe)
        self.assertIn('formats: ["int16u", "rational64u"]', rendered)
        self.assertNotIn('int8u', rendered)
        self.assertNotIn('int32u', rendered)
        rules = (ROOT / 'src/writers/generated_scalar_rules.rs').read_text()
        block = re.search(r'pub\(crate\) const NUMERIC_SCALAR:.*?^\}\);$', rules, re.MULTILINE | re.DOTALL)
        self.assertIsNotNone(block)
        public = block.group(0)
        self.assertIn('formats: ["int16u", "rational64u"]', public)
        self.assertIn('maxima: [65535, 4294967295]', public)
        self.assertNotIn('int8u', public)
        self.assertNotIn('int32u', public)

    def test_every_validation_statement_and_dependency_is_consumed(self):
        original = source()
        for dependency in (None, 'Image::ExifTool::IsInt', 'Image::ExifTool::IsHex', 'Image::ExifTool::IsFloat'):
            changed = copy.deepcopy(original)
            fact = changed['native_write_helpers']['check_value']
            if dependency:
                fact = fact['dependencies'][dependency]
            fact['__deparse'] = fact['__deparse'].replace('use strict;', 'use strict; ($_[0] += 1);', 1)
            with self.subTest(dependency=dependency), self.assertRaises(RecipeRefused):
                compile_numeric_scalar(changed)

    def test_source_binding_range_and_dependency_changes_refuse(self):
        for change in ('range', 'closure', 'binding', 'dependency', 'dispatch'):
            document = source()
            fact = document['native_write_helpers']['check_value']
            if change == 'range':
                fact['lexical_hashes']['bindings']['%intRange']['entries']['int16u'][1] = '65534'
            elif change == 'closure':
                fact['source_sha256'] = '0' * 64
            elif change == 'binding':
                fact['requested_binding'] = 'Image::ExifTool::Other'
            elif change == 'dependency':
                del fact['dependencies']['Image::ExifTool::IsInt']
            else:
                document['native_write_helpers']['write_value']['__deparse'] = document['native_write_helpers']['write_value']['__deparse'].replace('use strict;', 'use strict; ($val += 1);')
            with self.subTest(change=change), self.assertRaises(RecipeRefused):
                compile_numeric_scalar(document)

    def test_shared_rust_runtime_unsigned_fraction_and_rejection_boundaries(self):
        recipe = compile_numeric_scalar(source())
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            driver = directory / 'driver.rs'
            driver.write_text('''#![allow(dead_code)]
mod error {
#[derive(Debug)] pub struct ExifToolError;
impl ExifToolError { pub fn unsupported_format<T: Into<String>>(_: T) -> Self { Self } }
pub type Result<T> = std::result::Result<T, ExifToolError>;
}
#[path = "''' + str(ROOT / 'src/writers/generated_scalar.rs') + '''"] mod generated_scalar;
use generated_scalar::*;
''' + render_numeric(recipe) + '''
fn main() {
let recipe=NUMERIC_SCALAR.as_ref().unwrap();
for (format, text, little_hex, big_hex) in [
("int16u", "0", "0000", "0000"),
("int16u", "65535", "ffff", "ffff"),
("int16u", "258", "0201", "0102"),
("rational64u", "0", "0000000001000000", "0000000000000001"),
("rational64u", "300", "2c01000001000000", "0000012c00000001"),
("rational64u", "3/2", "0300000002000000", "0000000300000002"),
("rational64u", "1/0", "0100000000000000", "0000000100000000"),
("rational64u", "4294967295/4294967295", "ffffffffffffffff", "ffffffffffffffff"),
("rational64u", "1.5", "0300000002000000", "0000000300000002"),
("rational64u", "1,5", "0300000002000000", "0000000300000002"),
("rational64u", "1e3", "e803000001000000", "000003e800000001"),
("rational64u", "4294967296", "ffffffff01000000", "ffffffff00000001"),
("rational64u", "4294967296/1", "0000000001000000", "0000000000000001"),
("rational64u", "3.14159265358979", "9c970100bf810000", "0001979c000081bf"),
("int16u", "1.5", "0200", "0002"),
("int16u", "1e3", "e301", "01e3"),
("int16u", "-0.49", "0000", "0000"),
("int16u", "0xfeedfeed", "edfe", "feed")
] {
 for (little, expected) in [(true,little_hex),(false,big_hex)] {
  let bytes=serialize_numeric(recipe,&Scalar::Utf8(text.into()),format,little).unwrap();
  assert_eq!(bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),expected);
 }
}
for (format,text) in [("int16u","65536"),("int16u","-0.5"),("rational64u","-1"),("rational64u","1/-2"),("rational64u"," 72"),("int16u","3/2"),("rational64u","1/2/3")] {
 assert!(serialize_numeric(recipe,&Scalar::Utf8(text.into()),format,true).is_err(),"accepted {format} {text}");
}
assert!(numeric_value(recipe,&Scalar::Utf8("1".into()),"int16u",Some(2)).is_err());
}
''')
            binary = directory / 'driver'
            subprocess.run(['rustc', '--edition=2024', str(driver), '-o', str(binary)], check=True, capture_output=True, text=True)
            subprocess.run([str(binary)], check=True, capture_output=True, text=True)

if __name__ == '__main__':
    unittest.main()
