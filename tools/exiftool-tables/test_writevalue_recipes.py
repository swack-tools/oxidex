"""Source-change admission and scalar reference execution for WriteValue."""
from pathlib import Path
import json
import os
import subprocess
import unittest

from checkexif_recipes import RecipeRefused
from checkvalue_recipes import NativeScalar
from writevalue_recipes import compile_scalar_write, evaluate_scalar_write

BODY = (Path(__file__).parent / "testdata/writevalue_scalar_body.txt").read_text()


def fact(body=BODY, entries=None):
    return {"__perl": "CODE", "resolved": True,
            "requested_binding": "Image::ExifTool::WriteValue",
            "__name": "Image::ExifTool::WriteValue",
            "source_file": "Image/ExifTool/Writer.pl", "source_sha256": "b" * 64,
            "__deparse": body, "dependencies": {},
            "lexical_hashes": {"resolved": True, "bindings": {
                "%writeValueProc": {"resolved": True, "entries": entries or {}}}}}


class ScalarWriteTests(unittest.TestCase):
    def setUp(self):
        self.recipe = compile_scalar_write(fact())

    def test_count_and_scalar_kind_are_preserved(self):
        cases = (
            (NativeScalar("undefined", None), "undef", None, NativeScalar("undefined", None), 0),
            (NativeScalar("undefined", None), "string", None, NativeScalar("bytes", b"\0"), 1),
            (NativeScalar("bytes", b"a\0b"), "undef", 0, NativeScalar("bytes", b"a\0b"), 3),
            (NativeScalar("utf8", "é"), "string", -1, NativeScalar("utf8", "é\0"), 2),
            (NativeScalar("bytes", b"a"), "string", 4, NativeScalar("bytes", b"a\0\0\0"), 4),
            (NativeScalar("utf8", "é"), "undef", 3, NativeScalar("utf8", "é\0\0"), 3),
        )
        for value, format_name, count, expected_value, expected_count in cases:
            with self.subTest(value=value, format=format_name, count=count):
                actual = evaluate_scalar_write(self.recipe, value, format_name, count)
                self.assertEqual((actual.value, actual.count), (expected_value, expected_count))

    def test_source_truncation_uses_terminated_format_and_count(self):
        self.assertEqual(evaluate_scalar_write(self.recipe, NativeScalar("bytes", b"abcd"), "string", 3).value,
                         NativeScalar("bytes", b"ab\0"))
        self.assertEqual(evaluate_scalar_write(self.recipe, NativeScalar("bytes", b"abcd"), "undef", 3).value,
                         NativeScalar("bytes", b"abc"))

    def test_source_format_change_changes_admission_and_execution(self):
        changed = compile_scalar_write(fact(BODY.replace("'string'", "'wide-string'")))
        with self.assertRaises(RecipeRefused):
            evaluate_scalar_write(changed, NativeScalar("bytes", b"a"), "string", 3)
        self.assertEqual(evaluate_scalar_write(changed, NativeScalar("bytes", b"a"), "wide-string", 3).value,
                         NativeScalar("bytes", b"a\0\0"))

    def test_changed_source_count_bound_changes_serialization(self):
        changed = compile_scalar_write(fact(BODY.replace("$count > 0", "$count > 2")))
        value = NativeScalar("bytes", b"abcd")
        self.assertEqual(evaluate_scalar_write(self.recipe, value, "string", 2).value,
                         NativeScalar("bytes", b"a\0"))
        result = evaluate_scalar_write(changed, value, "string", 2)
        self.assertEqual((result.value, result.count), (NativeScalar("bytes", b"abcd\0"), 5))

    def test_live_dispatch_interception_or_unresolved_capture_refuses(self):
        with self.assertRaises(RecipeRefused):
            compile_scalar_write(fact(entries={"string": {"__perl": "CODE"}}))
        missing = fact()
        missing["lexical_hashes"]["resolved"] = False
        with self.assertRaises(RecipeRefused):
            compile_scalar_write(missing)
        unresolved = fact()
        unresolved["lexical_hashes"]["bindings"]["%writeValueProc"]["resolved"] = False
        with self.assertRaises(RecipeRefused):
            compile_scalar_write(unresolved)

    def test_control_flow_entry_and_scalar_branch_mutations_refuse(self):
        mutations = (
            BODY.replace("if ($proc)", "if (1)"),
            BODY.replace("    elsif", "    if", 1),
            BODY.replace("my($proc)", "my($format)"),
            BODY.replace("my($diff)", "my($count)").replace("$diff", "$count"),
            BODY.replace("$diff", "$val"),
            BODY.replace("(return $val);", "do_something(); (return $val);"),
            BODY.replace("$count > 0", "$count >= 0"),
        )
        for body in mutations:
            with self.subTest(body=body), self.assertRaises(RecipeRefused):
                compile_scalar_write(fact(body))

    def test_non_scalar_formats_and_noninteger_counts_refuse(self):
        with self.assertRaises(RecipeRefused):
            evaluate_scalar_write(self.recipe, NativeScalar("bytes", b"1"), "int8u", 1)
        for count in (True, 1.5, "3"):
            with self.subTest(count=count), self.assertRaises(RecipeRefused):
                evaluate_scalar_write(self.recipe, NativeScalar("bytes", b"a"), "string", count)


@unittest.skipUnless(os.environ.get("OXIDEX_PINNED_EXIFTOOL") and
                     os.environ.get("OXIDEX_TABLES_JSON"),
                     "requires explicit pinned native source and matching captured tables")
class NativeScalarDifferential(unittest.TestCase):
    def test_actual_capture_refuses_a_mutated_live_dispatch(self):
        document = json.loads(Path(os.environ["OXIDEX_TABLES_JSON"]).read_text())
        helper = document["native_write_helpers"]["write_value"]
        self.assertEqual(compile_scalar_write(helper).dispatch_lexical, "%writeValueProc")
        mutated = json.loads(json.dumps(helper))
        mutated["lexical_hashes"]["bindings"]["%writeValueProc"]["entries"]["string"] = {"__perl": "CODE"}
        with self.assertRaises(RecipeRefused):
            compile_scalar_write(mutated)

    def test_actual_native_helper_matches_scalar_states_and_counts(self):
        document = json.loads(Path(os.environ["OXIDEX_TABLES_JSON"]).read_text())
        recipe = compile_scalar_write(document["native_write_helpers"]["write_value"])
        values = [NativeScalar("undefined", None)]
        values += [NativeScalar("bytes", value) for value in
                   (b"", b"A", b"abc", b"a\0b", b"\0", b"\xff", b"\xc3\xa9", b"line\n")]
        values += [NativeScalar("utf8", value) for value in
                   ("", "é", "Ā", "😀", "a\0b", "a\n", "Aé", "AĀ")]
        cases, expected = [], []
        for value in values:
            for format_name in recipe.formats:
                for count in (None, 0, -1, 1, 2, 3, 4, 8):
                    cases.append({"kind": value.kind,
                                  "value": value.value.hex() if value.kind == "bytes" else value.value,
                                  "format": format_name, "count": count})
                    result = evaluate_scalar_write(recipe, value, format_name, count)
                    raw = result.value.value.encode("utf8") if result.value.kind == "utf8" else result.value.value
                    expected.append({"defined": result.value.kind != "undefined",
                                     "utf8": result.value.kind == "utf8",
                                     "hex": None if raw is None else raw.hex(),
                                     "length": None if result.value.value is None else len(result.value.value),
                                     # Perl's lexical ``my $count`` is not an
                                     # alias of the caller's scalar.  The
                                     # helper's final count is retained by the
                                     # reference result, while this oracle can
                                     # only observe that the caller's count is
                                     # unchanged.
                                     "count": count})
        perl = r'''
use strict; use warnings; use JSON::PP; use Encode (); use B (); use B::Deparse;
use Digest::SHA qw(sha256_hex);
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use Image::ExifTool;
require 'Image/ExifTool/Writer.pl'; local $/;
my $rows=JSON::PP->new->utf8->decode(<STDIN>); my @results; my $cv=\&Image::ExifTool::WriteValue;
my $body=B::Deparse->new('-p','-sC')->coderef2text($cv);
open my $source, '<:raw', B::svref_2object($cv)->FILE or die $!; my $source_sha=sha256_hex(<$source>); close $source;
for my $r (@$rows) {
 my $value=$r->{kind} eq 'undefined' ? undef : $r->{kind} eq 'bytes' ? pack('H*',$r->{value}) : $r->{value};
 utf8::upgrade($value) if $r->{kind} eq 'utf8'; my $count=$r->{count};
 my $out=Image::ExifTool::WriteValue($value,$r->{format},$count); my $flag=utf8::is_utf8($out)?JSON::PP::true:JSON::PP::false;
 my $bytes=defined($out)?($flag ? Encode::encode('UTF-8',$out) : $out):undef;
 push @results,{defined=>defined($out)?JSON::PP::true:JSON::PP::false,utf8=>$flag,hex=>defined($bytes)?unpack('H*',$bytes):undef,length=>defined($out)?length($out):undef,count=>$count};
}
print JSON::PP->new->canonical->utf8->encode({results=>\@results,source_sha256=>$source_sha,body_sha256=>sha256_hex($body)});
'''
        env = os.environ.copy()
        for name in ("PERL5LIB", "PERLLIB", "PERL5OPT"):
            env.pop(name, None)
        native_root = Path(os.environ["OXIDEX_PINNED_EXIFTOOL"])
        lib = native_root / "lib" if (native_root / "lib").is_dir() else native_root
        self.assertTrue((lib / "Image/ExifTool/Writer.pl").is_file())
        result = subprocess.run([env.get("EXIFTOOL_PERL", "/usr/bin/perl"),
                                 "-I" + str(lib), "-e", perl],
                                input=json.dumps(cases, ensure_ascii=False).encode("utf8"), env=env,
                                capture_output=True, timeout=30, check=True)
        native = json.loads(result.stdout)
        self.assertEqual(native["source_sha256"], recipe.provenance.source_sha256)
        self.assertEqual(native["body_sha256"], recipe.provenance.body_sha256)
        self.assertEqual(native["results"], expected)


if __name__ == "__main__":
    unittest.main()
