"""Fresh native source -> compiler -> actual Rust Sanitize differential proof."""
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import uuid

import checkexif_native_differential as native
import sanitize_rust_codegen as generator
from scalar_helper_codegen import rust_string

ROOT = Path(__file__).resolve().parents[2]
NATIVE = r'''
BEGIN { $Image::ExifTool::configFile = ''; }
use strict; use warnings; use JSON::PP; use Encode (); use B::Deparse;
use Digest::SHA qw(sha256_hex);
use Image::ExifTool;
require 'Image/ExifTool/Exif.pm'; require 'Image/ExifTool/WriteExif.pl';
require 'Image/ExifTool/Writer.pl';
local $/; my $cases = JSON::PP->new->utf8->decode(<STDIN>);
my $body = B::Deparse->new('-p','-sC')->coderef2text(\&Image::ExifTool::Sanitize);
open my $fh, '<:raw', $INC{'Image/ExifTool/Writer.pl'} or die $!;
my $source = <$fh>; close $fh;
my @results;
for my $case (@$cases) {
    my $kind = $case->{scalar}{kind};
    my $inner = $kind eq 'undefined' ? undef : $kind eq 'bytes'
        ? pack('H*', $case->{scalar}{hex}) : $case->{scalar}{text};
    utf8::upgrade($inner) if $kind eq 'utf8';
    my $value = $case->{reference} ? \$inner : $inner;
    my $tool = Image::ExifTool->new;
    $tool->Options(Escape => $case->{escape});
    # Set the helper input explicitly even in releases that never consult it.
    $tool->{OPTIONS}{EncodeHangs} = $case->{encode_hangs};
    Image::ExifTool::Sanitize($tool, \$value);
    my $flag = defined($value) && utf8::is_utf8($value);
    push @results, !defined($value) ? {kind=>'undefined'}
        : $flag ? {kind=>'utf8',text=>$value}
        : {kind=>'bytes',hex=>unpack('H*',$value)};
}
print JSON::PP->new->canonical->utf8->encode({body_sha256=>sha256_hex($body),
    source_sha256=>sha256_hex($source), results=>\@results});
'''


def rust_scalar(value):
    if value["kind"] == "undefined":
        return "Scalar::Undefined"
    if value["kind"] == "bytes":
        return "Scalar::Bytes(vec![" + ",".join(str(n) for n in bytes.fromhex(value["hex"])) + "])"
    return "Scalar::Utf8(" + rust_string(value["text"]) + ".to_owned())"


@unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                     "requires explicit native interpreter and selected ExifTool source")
class NativeSanitizeRust(unittest.TestCase):
    def test_source_guard_changes_reach_actual_generated_rust(self):
        perl = native.resolve_perl(Path(os.environ["EXIFTOOL_PERL"]))
        library = native.resolve_library(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]))
        values = [{"kind": "undefined"}]
        values += [{"kind": "bytes", "hex": value} for value in ("", "ff", "c3a9", "610062")]
        values += [{"kind": "utf8", "text": value} for value in ("", "a", "é", "€", "😀", "a\0b", "\0", "é\0")]
        base_cases = [{"scalar": value, "reference": reference, "escape": escape,
                       "encode_hangs": False}
                 for value in values for reference in (False, True) for escape in (0, "other")]
        env = {key: value for key, value in os.environ.items()
               if not (key.startswith("PERL5") or key in {"PERLLIB", "OXIDEX_UTF8_EVIDENCE_DIR"})}
        destination = os.environ.get("OXIDEX_SANITIZE_PROOF_DIR")
        if destination:
            directory = Path(destination) / ("native-rust-" + uuid.uuid4().hex)
            directory.mkdir(parents=True)
            context = nullcontext(str(directory))
        else:
            context = tempfile.TemporaryDirectory(prefix="oxidex-sanitize-rust-")
        variants = {}
        with context as temporary:
            for variant in ("canonical", "guard_changed", "without_encode_hangs"):
                work = Path(temporary) / variant
                work.mkdir()
                selected = library
                if variant != "canonical":
                    selected = work / "lib"
                    shutil.copytree(library, selected)
                    source = selected / "Image/ExifTool/Writer.pl"
                    text = source.read_text()
                    begin = text.index("sub Sanitize($$)\n{")
                    end = text.index("\n#------------------------------------------------------------------------------", begin)
                    body = text[begin:end]
                    if variant == "guard_changed":
                        self.assertEqual(body.count("$] >= 5.006"), 1)
                        body = body.replace("$] >= 5.006", "$] >= 10.006")
                    else:
                        operand = "$$self{OPTIONS}{EncodeHangs} or"
                        count = body.count(operand)
                        # Older actual releases already lack both guards; do
                        # not count a duplicate unchanged copy as new evidence.
                        if count == 0:
                            continue
                        self.assertEqual(count, 2)
                        body = body.replace(operand, "")
                    source.write_text(text[:begin] + body + text[end:])
                captured = subprocess.run([str(perl), str(ROOT / "tools/exiftool-tables/dump_tables.pl"),
                                           str(selected), "Exif"], env=env, capture_output=True,
                                          timeout=60, check=True)
                (work / "capture.json").write_bytes(captured.stdout)
                (work / "capture.stderr").write_bytes(captured.stderr)
                document = json.loads(captured.stdout)
                rendered, report = generator.generate(document)
                self.assertEqual(report["omissions"], [])
                self.assertIsNotNone(report["recipe"])
                if variant == "without_encode_hangs":
                    self.assertFalse(report["recipe"]["encode_hangs_guard"])
                # A source without EncodeHangs ignores that option; a source
                # whose version guard is false does not reach it. Exercise
                # both option values whenever no unsupported manual path runs.
                manual_reachable = (report["recipe"]["encode_hangs_guard"]
                                    and report["perl_version"] >= report["recipe"]["downgrade_at_or_after"])
                cases = list(base_cases)
                if not manual_reachable:
                    cases += [dict(case, encode_hangs=True) for case in base_cases]
                (work / "report.json").write_text(json.dumps(report, indent=2) + "\n")
                (work / "rules.rs").write_text(rendered)
                observed = subprocess.run([str(perl), "-I" + str(selected), "-e", NATIVE],
                                          input=json.dumps(cases, ensure_ascii=False).encode(),
                                          env=env, capture_output=True, timeout=30, check=True)
                (work / "native.json").write_bytes(observed.stdout)
                (work / "native.stderr").write_bytes(observed.stderr)
                actual = json.loads(observed.stdout)
                provenance = report["recipe"]["provenance"]
                self.assertEqual(actual["body_sha256"], provenance["body_sha256"])
                self.assertEqual(actual["source_sha256"], provenance["source_sha256"])
                self.assertEqual(actual["source_sha256"], hashlib.sha256((selected / "Image/ExifTool/Writer.pl").read_bytes()).hexdigest())
                self.assertEqual(len(actual["results"]), len(cases))
                variants[variant] = actual["results"][:len(base_cases)]
                (work / "cases.json").write_text(json.dumps(cases, ensure_ascii=False, indent=2) + "\n")
                harness = ["#![allow(dead_code)]",
                    '#[path=' + rust_string(str(ROOT / "src/error/mod.rs")) + '] mod error;',
                    'mod writers {',
                    '#[path=' + rust_string(str(ROOT / "src/writers/generated_scalar.rs")) + '] pub(crate) mod generated_scalar;',
                    '#[path=' + rust_string(str(ROOT / "src/writers/generated_sanitize.rs")) + '] pub(crate) mod generated_sanitize; }',
                    '#[path=' + rust_string(str(work / "rules.rs")) + '] mod rules;',
                    'use writers::generated_scalar::Scalar; use writers::generated_sanitize::*;',
                    '#[test] fn native_cases() { let recipe = rules::SANITIZE_RECIPE.as_ref().unwrap();']
                for index, (case, result) in enumerate(zip(cases, actual["results"], strict=True)):
                    reference = "ScalarReference" if case["reference"] else "Direct"
                    escape = "OtherTruthy" if case["escape"] else "Disabled"
                    encode_hangs = str(case["encode_hangs"]).lower()
                    harness.append(f'assert_eq!(sanitize(recipe, SanitizeInput::{reference}(' + rust_scalar(case["scalar"]) +
                                   f'), SanitizeOptions {{encode_hangs:{encode_hangs},escape:EscapeOption::{escape}}}).unwrap(), ' +
                                   rust_scalar(result) + f', "native case {index}");')
                for escape in ("Xml", "Html"):
                    harness.append('assert!(sanitize(recipe, SanitizeInput::Direct(Scalar::Bytes(vec![])), '
                                   f'SanitizeOptions {{encode_hangs:false,escape:EscapeOption::{escape}}}).is_err());')
                harness.append('assert!(sanitize(recipe, SanitizeInput::OtherReference, SanitizeOptions '
                               '{encode_hangs:false,escape:EscapeOption::Disabled}).is_err());')
                if manual_reachable:
                    harness.append('assert!(sanitize(recipe, SanitizeInput::Direct(Scalar::Utf8("é".into())), '
                                   'SanitizeOptions {encode_hangs:true,escape:EscapeOption::Disabled}).is_err());')
                harness.append('}')
                program = work / "proof.rs"
                program.write_text("\n".join(harness))
                for command, label in ((["rustc", "--edition=2024", "--test", str(program), "-o", str(work / "proof")], "compile"),
                                       ([str(work / "proof"), "--nocapture"], "execute")):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=60)
                    (work / (label + ".log")).write_text(result.stdout + result.stderr)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertNotEqual(variants["canonical"], variants["guard_changed"])


if __name__ == "__main__":
    unittest.main()
