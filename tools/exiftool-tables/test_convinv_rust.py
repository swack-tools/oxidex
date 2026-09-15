"""Native HostComputer ConvInv rows through fresh generated Rust static rows."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import checkexif_rust_codegen as check_codegen
import convinv_row_codegen as row_codegen
import convinv_rust_codegen as conv_codegen
from checkexif_native_differential import resolve_library, resolve_perl
from scalar_helper_codegen import rust_string


ROOT = Path(__file__).resolve().parents[2]
HOST_ID = "316"
NATIVE = r'''BEGIN{$Image::ExifTool::configFile=''}use strict;use warnings;use JSON::PP;use Encode();use Image::ExifTool;use Image::ExifTool::Exif;require 'Image/ExifTool/Writer.pl';my$t=$ENV{OXIDEX_ROW_NAME}or die'row name';my$e=Image::ExifTool->new;my$h=$e->GetTagInfo(\%Image::ExifTool::Exif::Main,0x013c)or die'HostComputer';my@o;for my$r(@{JSON::PP->new->utf8->decode(do{local$/;<STDIN>})}){my$v=$r->{u}?undef:$r->{utf8}?$r->{v}:pack('H*',$r->{v});utf8::upgrade($v)if$r->{utf8};my($z,$err)=$e->ConvInv($v,$h,$t,'IFD0');my$u=defined$z&&utf8::is_utf8($z);push@o,{name=>$r->{name},defined=>defined$z?JSON::PP::true:JSON::PP::false,utf8=>$u?JSON::PP::true:JSON::PP::false,hex=>defined$z?unpack('H*',$u?Encode::encode('UTF-8',$z):$z):undef,error=>$err};}print JSON::PP->new->canonical->encode(\@o);'''


def scalar(case):
    if case.get("u"):
        return "Scalar::Undefined"
    if case.get("utf8"):
        return "Scalar::Utf8(" + rust_string(case["v"]) + ".to_owned())"
    return "Scalar::Bytes(vec![" + ",".join(str(item) for item in bytes.fromhex(case["v"])) + "])"


def expected(native):
    if not native["defined"]:
        value = "Scalar::Undefined"
    elif native["utf8"]:
        value = "Scalar::Utf8(" + rust_string(bytes.fromhex(native["hex"]).decode()) + ".to_owned())"
    else:
        value = "Scalar::Bytes(vec![" + ",".join(str(item) for item in bytes.fromhex(native["hex"])) + "])"
    error = "None" if native["error"] is None else "Some(" + rust_string(native["error"]) + ".to_owned())"
    return value, error


def run_native(test, perl, library, cases, row_name):
    env = {key: value for key, value in os.environ.items() if key not in {"PERL5LIB", "PERLLIB", "PERL5OPT"}}
    env["OXIDEX_ROW_NAME"] = row_name
    result = subprocess.run([str(perl), "-I" + str(library), "-e", NATIVE], input=json.dumps(cases, ensure_ascii=False).encode(),
                            env=env, capture_output=True, timeout=30)
    test.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
    return json.loads(result.stdout)


def rust_matches(test, cases, native, check_rules, conv_rules, rows, directory, row_name, *, allow_refusal=False):
    directory.mkdir()
    paths = {"rules": directory / "rules.rs", "conv": directory / "conv.rs", "rows": directory / "rows.rs"}
    paths["rules"].write_text(check_rules)
    paths["conv"].write_text(conv_rules)
    paths["rows"].write_text(rows)
    source = ROOT / "src/writers"
    lines = [
        "#![allow(dead_code)]",
        "#[path=" + rust_string(str(ROOT / "src/error/mod.rs")) + "]mod error;",
        "mod writers{",
        "#[path=" + rust_string(str(source / "generated_scalar.rs")) + "]pub(crate)mod generated_scalar;",
        "#[path=" + rust_string(str(source / "generated_scalar_rules.rs")) + "]pub(crate)mod generated_scalar_rules;",
        "#[path=" + rust_string(str(source / "generated_checkexif.rs")) + "]pub(crate)mod generated_checkexif;",
        "#[path=" + rust_string(str(source / "generated_convinv.rs")) + "]pub(crate)mod generated_convinv;}",
        "#[path=" + rust_string(str(paths["rules"])) + "]mod rules;",
        "#[path=" + rust_string(str(paths["conv"])) + "]mod conv_rules;",
        "#[path=" + rust_string(str(paths["rows"])) + "]mod rows;",
        "use writers::generated_scalar::*;use writers::generated_convinv::*;",
        "#[test]fn proof(){let recipe=conv_rules::CONV_INV_RECIPE.as_ref().expect(\"supported source\");",
        "let row=rows::CONV_INV_ROWS.iter().find(|row|row.module==\"Exif\"&&row.table==\"Main\"&&row.full_name==\"Image::ExifTool::Exif::Main\"&&row.raw_id==\"316\"&&row.name==" + rust_string(row_name) + ").expect(\"native selected row\");",
    ]
    for case, output in zip(cases, native, strict=True):
        value, error = expected(output)
        call = "conv_inv_static(recipe," + scalar(case) + ",row,rules::CHECK_EXIF_RECIPES,None,None)"
        if allow_refusal:
            lines.append("match " + call + " { Ok(got)=>{assert_eq!(got.value," + value + "," + rust_string(case["name"]) + ");assert_eq!(got.error," + error + "," + rust_string(case["name"] + " error") + ");},Err(error)=>assert!(error.to_string().contains(\"refused\")), }")
        else:
            lines.append("let got=" + call + ".unwrap();")
            lines.append("assert_eq!(got.value," + value + "," + rust_string(case["name"]) + ");")
            lines.append("assert_eq!(got.error," + error + "," + rust_string(case["name"] + " error") + ");")
    lines.append("}")
    proof = directory / "proof.rs"
    proof.write_text("\n".join(lines))
    built = subprocess.run(["rustc", "--edition=2024", "--test", str(proof), "-o", str(directory / "proof")],
                           capture_output=True, text=True, timeout=60)
    test.assertEqual(built.returncode, 0, built.stderr)
    ran = subprocess.run([str(directory / "proof")], capture_output=True, text=True, timeout=30)
    test.assertEqual(ran.returncode, 0, ran.stderr)


def dump(test, perl, library):
    env = {key: value for key, value in os.environ.items() if key not in {"PERL5LIB", "PERLLIB", "PERL5OPT"}}
    result = subprocess.run([str(perl), str(ROOT / "tools/exiftool-tables/dump_tables.pl"), str(library), "Exif"],
                            env=env, capture_output=True, timeout=60)
    test.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
    return json.loads(result.stdout)


@unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                     "requires explicit canonical Perl and pinned library")
class ConvInvRustTest(unittest.TestCase):
    cases = [{"name": "bytes_nul", "v": "610062"}, {"name": "utf8", "v": "é", "utf8": 1},
             {"name": "empty", "v": ""}, {"name": "undef", "u": 1}, {"name": "long", "v": "616263646566"}]

    def generated_native_proof(self, library, *, row_name="HostComputer", allow_refusal=False):
        perl = resolve_perl(Path(os.environ["EXIFTOOL_PERL"]))
        document = dump(self, perl, library)
        native = run_native(self, perl, library, self.cases, row_name)
        check_rules, check_report = check_codegen.generate(document)
        conv_rules, conv_report = conv_codegen.generate(document)
        rows, row_report = row_codegen.generate(document)
        self.assertTrue(check_report["recipes"])
        self.assertEqual(check_report["recipes"][0]["check_proc"]["actual_name"],
                         "Image::ExifTool::Exif::CheckExif")
        self.assertTrue(conv_report["emitted"])
        self.assertTrue(row_report["rows_emitted"])
        self.assertIn('raw_id: "316"', rows)
        with tempfile.TemporaryDirectory(prefix="oxidex-convinv-static-") as temporary:
            rust_matches(self, self.cases, native, check_rules, conv_rules, rows,
                         Path(temporary) / "rust", row_name, allow_refusal=allow_refusal)
        return document, rows, row_report

    def test_actual_native_hostcomputer_row_captures_and_executes(self):
        library = resolve_library(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]))
        document, rows, report = self.generated_native_proof(library)
        host = document["native_write_tables"]["Exif"]["Main"]["rows"][HOST_ID]
        self.assertEqual(host["properties"]["Name"]["value"], "HostComputer")
        self.assertIn("HostComputer", rows)
        self.assertEqual(report["runtime_status"], "inactive; static source rows are not connected to public writing")

    def test_copied_native_hostcomputer_mutation_propagates_to_static_row_or_refuses(self):
        library = resolve_library(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]))
        with tempfile.TemporaryDirectory(prefix="oxidex-convinv-hostcomputer-") as temporary:
            changed = Path(temporary) / "lib"
            shutil.copytree(library, changed)
            source = changed / "Image/ExifTool/Exif.pm"
            original = source.read_text()
            needle = "Name => 'HostComputer',\n        Writable => 'string',"
            self.assertEqual(original.count(needle), 1)
            source.write_text(original.replace(
                needle,
                "Name => 'HostComputerChanged',\n        Writable => 'int16u',\n        Count => 1,"))
            document, rows, report = self.generated_native_proof(
                changed, row_name="HostComputerChanged", allow_refusal=True)
            host = document["native_write_tables"]["Exif"]["Main"]["rows"][HOST_ID]
            if any(item["raw_id"] == HOST_ID for item in report["omitted_rows"]):
                self.assertTrue(any(item["raw_id"] == HOST_ID and "unsupported" in item["reason"]
                                    for item in report["omitted_rows"]))
            else:
                self.assertEqual(host["properties"]["Name"]["value"], "HostComputerChanged")
                self.assertEqual(host["properties"]["Writable"]["value"], "int16u")
                self.assertEqual(host["properties"]["Count"]["value"], "1")
                self.assertIn('name: "HostComputerChanged"', rows)
                self.assertIn('PropertyValue::Text("int16u")', rows)
                self.assertIn('PropertyValue::Integer(1)', rows)


if __name__ == "__main__":
    unittest.main()
