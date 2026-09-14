"""Portable actual-module joins; no Cargo or native process is required."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

class NumericRuntimeComposition(unittest.TestCase):
    def test_actual_address_descriptor_and_final_runtime_modules(self):
        writer_modules = (
            'generated_scalar', 'generated_scalar_rules',
            'generated_setnewvalue_address_rules',
            'generated_setnewvalue_public_migration_rules',
            'generated_write_address', 'tiff_scalar_final_stage',
            'generated_tiff_scalar_final_rules',
        )
        source = '''#![allow(dead_code)]
mod error {
#[derive(Debug)] pub struct ExifToolError;
impl ExifToolError { pub fn unsupported_format<T: Into<String>>(_: T) -> Self { Self } }
pub type Result<T> = std::result::Result<T, ExifToolError>;
}
mod core { #[derive(PartialEq)] pub enum FormatFamily { EXIF } }
mod writers {
'''
        source += '\n'.join(f'#[path = "{ROOT}/src/writers/{name}.rs"] pub(crate) mod {name};' for name in writer_modules)
        source += '\n}\nmod tag_db {\n'
        source += f'#[path = "{ROOT}/src/tag_db/generated_scalar_descriptor_fallback.rs"] mod generated_scalar_descriptor_fallback;\n}}\n'
        with tempfile.TemporaryDirectory() as directory:
            driver, binary = Path(directory) / 'driver.rs', Path(directory) / 'driver'
            driver.write_text(source)
            built = subprocess.run(['rustc', '--edition=2024', '--test', str(driver), '-o', str(binary)], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stderr)
            ran = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)

if __name__ == '__main__':
    unittest.main()
