"""Native-source admission checks for the closed final scalar stage."""

import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest

sys.path.insert(0, str(Path(__file__).parent))
from final_scalar_stage import FinalStageRefused, compile_final_scalar_rows, compile_final_scalar_stage, generate, render_rust


ROOT = Path(__file__).resolve().parents[2]
DUMP = ROOT / "tools/exiftool-tables/dump_tables.pl"
PERL = Path(os.environ["EXIFTOOL_PERL"]) if os.environ.get("EXIFTOOL_PERL") else None
_native_root = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
LIB = (Path(_native_root) / "lib") if _native_root and (Path(_native_root) / "lib").is_dir() else (Path(_native_root) if _native_root else None)
NATIVE_READY = PERL is not None and LIB is not None and PERL.is_file() and LIB.is_dir()


def native_document(lib=None):
    if not NATIVE_READY:
        raise unittest.SkipTest("requires EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
    lib = lib or LIB
    result = subprocess.run([str(PERL), str(DUMP), str(lib), "Exif"], check=True, text=True, capture_output=True)
    document = json.loads(result.stdout)
    registry = r'''
use strict; use warnings; use JSON::PP; use Digest::SHA qw(sha256_hex);
BEGIN { $Image::ExifTool::configFile = ''; }
use Image::ExifTool; require 'Image/ExifTool/Exif.pm';
open my $fh, '<:raw', $INC{'Image/ExifTool/Exif.pm'} or die $!; local $/; my $sha=sha256_hex(<$fh>);
print JSON::PP->new->canonical->encode({ state=>'resolved', source=>{library_relative_path=>'Image/ExifTool/Exif.pm',sha256=>$sha}, format_name=>[map { defined $_ ? $_ : undef } @Image::ExifTool::Exif::formatName], format_size=>[map { defined $_ ? 0+$_ : undef } @Image::ExifTool::Exif::formatSize], format_number=>\%Image::ExifTool::Exif::formatNumber });
'''
    result = subprocess.run([str(PERL), "-I" + str(lib), "-e", registry], check=True, text=True, capture_output=True)
    document["native_write_format_registry"] = json.loads(result.stdout)
    return document


def native_final_scalar(lib=None):
    """Exercise the original WriteExif through a real minimal TIFF file."""
    from native_write_matrix import make_tiff, run_native
    lib = lib or LIB
    with TemporaryDirectory() as directory:
        directory = Path(directory)
        source, target = directory / "input.tif", directory / "output.tif"
        make_tiff(source, "big")
        native = run_native(PERL, lib, source, target, "utf8", "IFD0:HostComputer")
        if native["returncode"] != 0 or not native["result"] or not native["result"].get("write_return"):
            raise AssertionError(f"native WriteExif failed: {native}")
        # parse_tiff intentionally accepts only its default matrix types. The
        # raw directory read here extends that probe to TIFF UNDEFINED (7),
        # preserving the observed type/count/value without reimplementing a
        # native count formula.
        data = target.read_bytes()
        order = "big" if data[:2] == b"MM" else "little"
        offset = int.from_bytes(data[4:8], order)
        entries = int.from_bytes(data[offset:offset + 2], order)
        for position in range(offset + 2, offset + 2 + entries * 12, 12):
            if int.from_bytes(data[position:position + 2], order) != 0x013C:
                continue
            kind = int.from_bytes(data[position + 2:position + 4], order)
            count = int.from_bytes(data[position + 4:position + 8], order)
            width = {2: 1, 7: 1}[kind]
            slot = data[position + 8:position + 12]
            value = slot[:count * width] if count * width <= 4 else data[int.from_bytes(slot, order):int.from_bytes(slot, order) + count * width]
            return {"wire": kind, "count": count, "hex": value.hex()}
        raise AssertionError("native WriteExif did not create HostComputer entry")


def execute_rendered_recipe(document, recipe, raw):
    from scalar_helper_codegen import generate as generate_scalar_helpers
    generated_scalar = ROOT / "src/writers/generated_scalar.rs"
    final_stage = ROOT / "src/writers/tiff_scalar_final_stage.rs"
    recipes, _, registry = compile_final_scalar_stage(document)
    assert recipe in recipes
    with TemporaryDirectory() as directory:
        directory = Path(directory)
        rendered = directory / "rules.rs"
        rendered.write_text(render_rust(recipes, registry), encoding="utf-8")
        generated_rules = directory / "generated_scalar_rules.rs"
        generated_rules.write_text(generate_scalar_helpers(document)[0], encoding="utf-8")
        driver = directory / "driver.rs"
        driver.write_text(f'''mod error {{
#[derive(Debug)] pub struct ExifToolError; impl ExifToolError {{ pub fn unsupported_format<T: Into<String>>(_: T) -> Self {{ Self }} }} pub type Result<T> = std::result::Result<T, ExifToolError>;
}}
mod writers {{
#[path = "{generated_scalar}"] pub mod generated_scalar;
#[path = "{generated_rules}"] pub mod generated_scalar_rules;
#[path = "{final_stage}"] pub mod tiff_scalar_final_stage;
#[path = "{rendered}"] pub mod generated_final;
}}
use writers::generated_scalar::Scalar;
use writers::tiff_scalar_final_stage::*;
fn main() {{ let recipe=writers::generated_final::TIFF_SCALAR_FINAL_RECIPES.iter().find(|item| item.raw_tag_id == 0x013c).unwrap(); let registry=writers::generated_final::TIFF_SCALAR_FINAL_FORMAT_REGISTRY.as_ref().unwrap(); match resolve_tiff_scalar_final_stage(recipe, registry, Scalar::Bytes(vec!{list(raw)!r}), ScalarWriteOperation::Create, TiffByteOrder::BigEndian).unwrap() {{ ResolvedTiffScalarEdit::Write {{ wire_format, count, entry, .. }} => println!("{{wire_format}} {{count}} {{}}", entry[8..].iter().map(|v| format!("{{v:02x}}")).collect::<String>()), _ => panic!("expected write") }} }}
''', encoding="utf-8")
        binary = directory / "driver"
        subprocess.run(["rustc", "--edition=2021", str(driver), "-o", str(binary)], check=True, text=True, capture_output=True)
        wire, count, inline = subprocess.run([str(binary)], check=True, text=True, capture_output=True).stdout.split()
        return {"wire": int(wire), "count": int(count), "hex": inline}


@unittest.skipUnless(NATIVE_READY, "requires EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
class FinalScalarStageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = native_document()

    def host_recipe(self, document=None):
        recipes = compile_final_scalar_rows(document or self.document)
        return next(recipe for recipe in recipes if recipe.raw_tag_id == 0x013C)

    def test_actual_final_loaded_source_emits_host_from_properties_not_tag_allowlist(self):
        recipe = self.host_recipe()
        self.assertEqual((recipe.name, recipe.conversion_format, recipe.wire_format), ("HostComputer", "string", "string"))
        self.assertEqual(recipe.physical_write_group, "IFD0")
        self.assertEqual(recipe.raw_tag_id, 0x013C)
        _, _, registry = compile_final_scalar_stage(self.document)
        rendered = render_rust([recipe], registry)
        self.assertIn("raw_tag_id: 0x013c", rendered)
        self.assertNotIn("wire_format: 2", rendered)

    def test_context_is_required_and_prototype_spelling_is_not_normalized_away(self):
        changed = copy.deepcopy(self.document)
        changed.pop("native_write_capture_context")
        with self.assertRaisesRegex(FinalStageRefused, "capture_context"):
            compile_final_scalar_rows(changed)
        for symbol in ("Image::ExifTool::Canon::ReadODD", "Image::ExifTool::PanasonicRaw::PatchRawDataOffset", "Image::ExifTool::Sony::Decrypt"):
            changed = copy.deepcopy(self.document)
            fact = changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]
            self.assertIn("&" + symbol + "(", fact["__deparse"])
            fact["__deparse"] = fact["__deparse"].replace("&" + symbol + "(", symbol + "(", 1)
            with self.subTest(symbol=symbol), self.assertRaisesRegex(FinalStageRefused, "complete final-stage token grammar"):
                compile_final_scalar_rows(changed)

    def test_context_source_identities_cannot_be_mixed_with_other_compiler_inputs(self):
        for file in ("Image/ExifTool/WriteExif.pl", "Image/ExifTool/Writer.pl", "Image/ExifTool/Exif.pm"):
            changed = copy.deepcopy(self.document)
            changed["native_write_capture_context"]["loaded_modules"][file] = "0" * 64
            with self.subTest(file=file), self.assertRaisesRegex(FinalStageRefused, "compiled source differs"):
                compile_final_scalar_rows(changed)

    def test_copied_native_operand_change_refuses_before_generating_recipe(self):
        # This mutates a copied *native source file*, then asks the real dumper
        # for its final-loaded CV. It is not a convenient JSON/body mutation.
        with TemporaryDirectory() as directory:
            copied = Path(directory) / "lib"
            shutil.copytree(LIB, copied)
            writer = copied / "Image/ExifTool/WriteExif.pl"
            body = writer.read_text(encoding="utf-8")
            self.assertIn("WriteValue($newVal, $newFormName, $newCount)", body)
            writer.write_text(body.replace("WriteValue($newVal, $newFormName, $newCount)",
                                           "WriteValue($newVal, $newFormName, undef)", 1), encoding="utf-8")
            with self.assertRaises(FinalStageRefused):
                compile_final_scalar_rows(native_document(copied))

    def test_copied_native_final_count_rule_change_refuses_before_generation(self):
        with TemporaryDirectory() as directory:
            copied = Path(directory) / "lib"
            shutil.copytree(LIB, copied)
            writer = copied / "Image/ExifTool/WriteExif.pl"
            body = writer.read_text(encoding="utf-8")
            self.assertIn("int(($newSize + $fsize - 1) / $fsize)", body)
            writer.write_text(body.replace("int(($newSize + $fsize - 1) / $fsize)",
                                           "int($newSize / $fsize)", 1), encoding="utf-8")
            with self.assertRaises(FinalStageRefused):
                compile_final_scalar_rows(native_document(copied))

    def test_inserted_final_stage_statement_refuses_instead_of_becoming_hash_only_provenance(self):
        changed = copy.deepcopy(self.document)
        body = changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["__deparse"]
        changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["__deparse"] = body.replace(
            "($newValue = WriteValue($newVal, $newFormName, $newCount));",
            "($newValue = WriteValue($newVal, $newFormName, $newCount));\n                        injected_final_stage_action();", 1)
        with self.assertRaises(FinalStageRefused):
            compile_final_scalar_rows(changed)

    def test_inserted_new_value_assignment_refuses_before_execution(self):
        changed = copy.deepcopy(self.document)
        body = changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["__deparse"]
        changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["__deparse"] = body.replace(
            "($newValue = WriteValue($newVal, $newFormName, $newCount));",
            "($newValue = WriteValue($newVal, $newFormName, $newCount));\n                        ($newValue = \"CHANGED\");", 1)
        with self.assertRaises(FinalStageRefused):
            compile_final_scalar_rows(changed)

    def test_inserted_known_call_and_altered_layout_guard_refuse(self):
        body = self.document["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["__deparse"]
        cases = (
            body.replace("($newValue = WriteValue($newVal, $newFormName, $newCount));",
                         "warn('known-call');\n                        ($newValue = WriteValue($newVal, $newFormName, $newCount));", 1),
            body.replace("($newSize > 4)", "($newSize >= 4)", 1),
        )
        for changed_body in cases:
            with self.subTest(changed_body=changed_body != body):
                changed = copy.deepcopy(self.document)
                changed["native_write_tables"]["Exif"]["Main"]["effective_write_proc"]["effective"]["__deparse"] = changed_body
                with self.assertRaises(FinalStageRefused):
                    compile_final_scalar_rows(changed)

    def test_defined_empty_is_admitted_but_undefined_is_not_collapsed_in_source_facts(self):
        changed = copy.deepcopy(self.document)
        host = changed["native_write_tables"]["Exif"]["Main"]["rows"]["316"]
        host["properties"]["Format"] = {"present": True, "value": None}
        self.assertFalse(any(recipe.raw_tag_id == 0x013C for recipe in compile_final_scalar_rows(changed)))

    def test_registry_change_refuses_and_never_substitutes_a_rust_tiff_type(self):
        changed = copy.deepcopy(self.document)
        registry = changed["native_write_format_registry"]
        index = registry["format_number"]["string"]
        registry["format_name"][index] = "not-string"
        source, report = generate(changed)
        self.assertFalse(report["emitted"])
        self.assertIn("registry", report["reason"])
        self.assertIn("TIFF_SCALAR_FINAL_FORMAT_REGISTRY: Option<NativeTiffFormatRegistry> = None", source)

    def test_renderer_uses_shared_rust_literal_escaping(self):
        changed = copy.deepcopy(self.document)
        changed["native_write_tables"]["Exif"]["Main"]["rows"]["316"]["properties"]["Name"]["value"] = 'quote" slash\\ control\x01'
        recipes, _, registry = compile_final_scalar_stage(changed)
        rendered = render_rust([next(recipe for recipe in recipes if recipe.raw_tag_id == 0x013C)], registry)
        self.assertIn('tag_name: "quote\\" slash\\\\ control\\u{1}"', rendered)

    def test_actual_registry_emits_canonical_facts_and_aliases_with_provenance(self):
        recipes, _, registry = compile_final_scalar_stage(self.document)
        rendered = render_rust(recipes, registry)
        self.assertIn('NativeTiffFormatFact { name: "undef", number: 7, size: 1 }', rendered)
        self.assertIn('NativeTiffFormatAliasFact { name: "binary", number: 7 }', rendered)
        self.assertIn(registry.source_sha256, rendered)
        self.assertEqual(len({number for _, number, _ in registry.canonical_facts}), len(registry.canonical_facts))

    def test_copied_native_type_change_reaches_rendered_executor_and_matches_native(self):
        with TemporaryDirectory() as directory:
            copied = Path(directory) / "lib"
            shutil.copytree(LIB, copied)
            exif = copied / "Image/ExifTool/Exif.pm"
            source = exif.read_text(encoding="utf-8")
            needle = "Name => 'HostComputer',\n        Writable => 'string',"
            self.assertIn(needle, source)
            exif.write_text(source.replace(needle, "Name => 'HostComputer',\n        Writable => 'undef',", 1), encoding="utf-8")
            document = native_document(copied)
            recipe = self.host_recipe(document)
            registry = document["native_write_format_registry"]
            native = native_final_scalar(copied)
            generated = execute_rendered_recipe(document, recipe, bytes.fromhex("c3a9"))
            self.assertEqual((recipe.conversion_format, recipe.wire_format), ("undef", "undef"))
            self.assertEqual(generated, native | {"hex": native["hex"].ljust(8, "0")})

    def test_copied_native_format_size_change_uses_later_final_count_ceil(self):
        with TemporaryDirectory() as directory:
            copied = Path(directory) / "lib"
            shutil.copytree(LIB, copied)
            exif = copied / "Image/ExifTool/Exif.pm"
            source = exif.read_text(encoding="utf-8")
            self.assertIn("@formatSize = (undef,1,1,2,4,8,1,1", source)
            exif.write_text(source.replace("@formatSize = (undef,1,1,2,4,8,1,1", "@formatSize = (undef,1,2,2,4,8,1,1", 1), encoding="utf-8")
            document = native_document(copied)
            recipe = self.host_recipe(document)
            registry = document["native_write_format_registry"]
            native = native_final_scalar(copied)
            generated = execute_rendered_recipe(document, recipe, bytes.fromhex("c3a9"))
            self.assertEqual(native["count"], 2)
            self.assertEqual(generated, native | {"hex": native["hex"].ljust(8, "0")})


class StandaloneRustTests(unittest.TestCase):
    def test_final_executor_compiles_and_runs_without_cargo(self):
        generated = ROOT / "src/writers/generated_scalar.rs"
        final_stage = ROOT / "src/writers/tiff_scalar_final_stage.rs"
        with TemporaryDirectory() as directory:
            directory = Path(directory)
            driver = directory / "driver.rs"
            binary = directory / "driver"
            driver.write_text(f'''mod error {{
    #[derive(Debug)] pub struct ExifToolError;
    impl ExifToolError {{ pub fn unsupported_format<T: Into<String>>(_: T) -> Self {{ Self }} }}
    pub type Result<T> = std::result::Result<T, ExifToolError>;
}}
mod writers {{
    #[path = "{generated}"] pub mod generated_scalar;
    #[path = "{final_stage}"] pub mod tiff_scalar_final_stage;
}}
fn main() {{}}
''', encoding="utf-8")
            subprocess.run(["rustc", "--edition=2021", "--test", str(driver), "-o", str(binary)], check=True, text=True, capture_output=True)
            subprocess.run([str(binary)], check=True, text=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
