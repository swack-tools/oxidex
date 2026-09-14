"""Scalar semantics and source-change admission; native differential is separate."""
from pathlib import Path
import json
import os
import subprocess
import unittest

from checkexif_recipes import RecipeRefused
from checkvalue_recipes import NativeScalar, compile_scalar_check, evaluate_scalar_check

BODY = (Path(__file__).parent / "testdata/checkvalue_scalar_body.txt").read_text()


def fact(body=BODY):
    return {"__perl": "CODE", "resolved": True,
            "requested_binding": "Image::ExifTool::CheckValue",
            "__name": "Image::ExifTool::CheckValue",
            "source_file": "Image/ExifTool/Writer.pl", "source_sha256": "a" * 64,
            "__deparse": body, "dependencies": {}}


class ScalarCheckTests(unittest.TestCase):
    def setUp(self):
        self.recipe = compile_scalar_check(fact())

    def test_absent_zero_negative_counts_preserve_scalar(self):
        for value in (NativeScalar("undefined", None), NativeScalar("bytes", b""),
                      NativeScalar("bytes", b"a\0b"), NativeScalar("utf8", "é")):
            for count in (None, 0, -1, -3):
                for fmt in self.recipe.formats:
                    self.assertEqual(evaluate_scalar_check(self.recipe, value, fmt, count),
                                     (value, None))

    def test_string_and_undef_have_different_length_limits(self):
        value = NativeScalar("bytes", b"abc")
        self.assertEqual(evaluate_scalar_check(self.recipe, value, "string", 3),
                         (value, "String too long"))
        self.assertEqual(evaluate_scalar_check(self.recipe, value, "undef", 3), (value, None))
        self.assertEqual(evaluate_scalar_check(self.recipe, value, "undef", 2),
                         (value, "Data too long"))

    def test_padding_preserves_unicode_flag_and_nuls(self):
        cases = ((NativeScalar("bytes", b""), NativeScalar("bytes", b"\0\0\0")),
                 (NativeScalar("undefined", None), NativeScalar("bytes", b"\0\0\0")),
                 (NativeScalar("bytes", b"a\0"), NativeScalar("bytes", b"a\0\0")),
                 (NativeScalar("utf8", "é"), NativeScalar("utf8", "é\0\0")),
                 (NativeScalar("bytes", b"\xc3\xa9"), NativeScalar("bytes", b"\xc3\xa9\0")))
        for value, expected in cases:
            self.assertEqual(evaluate_scalar_check(self.recipe, value, "string", 3),
                             (expected, None))

    def test_source_operator_and_error_changes_change_execution(self):
        changed = compile_scalar_check(fact(BODY.replace("$len >= $count", "$len > $count")
                                              .replace("String too long", "Changed error")))
        value = NativeScalar("bytes", b"abc")
        self.assertEqual(evaluate_scalar_check(changed, value, "string", 3), (value, None))
        self.assertEqual(evaluate_scalar_check(changed, value, "string", 2),
                         (value, "Changed error"))

    def test_changed_format_follows_source(self):
        changed = compile_scalar_check(fact(BODY.replace("'string'", "'new-string'")))
        with self.assertRaises(RecipeRefused):
            evaluate_scalar_check(changed, NativeScalar("bytes", b"x"), "string", 2)
        self.assertEqual(evaluate_scalar_check(changed, NativeScalar("bytes", b"x"), "new-string", 2),
                         (NativeScalar("bytes", b"x\0"), None))

    def test_unproven_formats_and_count_types_refuse(self):
        with self.assertRaises(RecipeRefused):
            evaluate_scalar_check(self.recipe, NativeScalar("bytes", b"1"), "int8u", 1)
        for count in (True, 1.5, "3"):
            with self.assertRaises(RecipeRefused):
                evaluate_scalar_check(self.recipe, NativeScalar("bytes", b"x"), "string", count)

    def test_extra_statements_and_aliases_refuse(self):
        mutations = (BODY.replace("use strict;", "use strict; do_something();"),
                     BODY.replace("my(@vals, $val, $n);", "my(@vals, $count, $n);"),
                     BODY.replace("my($len)", "my($count)"),
                     BODY.replace("(return (undef));\n    }", "do_something(); (return (undef));\n    }"),
                     BODY.replace("and (return 'String too long')", "or (return 'String too long')"))
        for body in mutations:
            with self.subTest(body=body), self.assertRaises(RecipeRefused):
                compile_scalar_check(fact(body))

    def test_stale_binding_refuses(self):
        changed = fact()
        changed["requested_binding"] = "Image::ExifTool::Other"
        with self.assertRaises(RecipeRefused):
            compile_scalar_check(changed)


@unittest.skipUnless(os.environ.get("OXIDEX_PINNED_EXIFTOOL") and
                     os.environ.get("OXIDEX_TABLES_JSON"),
                     "requires explicit pinned native source and matching captured tables")
class NativeScalarDifferential(unittest.TestCase):
    def test_actual_native_helper_matches_all_scalar_states_and_counts(self):
        document = json.loads(Path(os.environ["OXIDEX_TABLES_JSON"]).read_text())
        recipe = compile_scalar_check(document["native_write_helpers"]["check_value"])
        native_root = Path(os.environ["OXIDEX_PINNED_EXIFTOOL"])
        lib = native_root / "lib"
        values = [NativeScalar("undefined", None)]
        values += [NativeScalar("bytes", value) for value in
                   (b"", b"A", b"abc", b"a\0b", b"\0", b"\xff", b"\xc3\xa9", b"line\n")]
        values += [NativeScalar("utf8", value) for value in ("", "é", "Ā", "😀", "a\0b", "a\n")]
        cases, expected = [], []
        for value in values:
            for fmt in ("string", "undef"):
                for count in (None, 0, -1, 1, 2, 3, 4, 8):
                    cases.append({"kind": value.kind, "value": value.value.hex()
                                  if value.kind == "bytes" else value.value,
                                  "format": fmt, "count": count})
                    result, error = evaluate_scalar_check(recipe, value, fmt, count)
                    raw = (result.value.encode("utf8") if result.kind == "utf8" else result.value)
                    expected.append({"defined": result.kind != "undefined",
                                     "utf8": result.kind == "utf8",
                                     "hex": None if raw is None else raw.hex(),
                                     "length": None if result.value is None else len(result.value),
                                     "error": error})
        perl = r'''
use strict; use warnings; use JSON::PP; use Encode (); use B (); use B::Deparse;
use Digest::SHA qw(sha256_hex); use Image::ExifTool;
require 'Image/ExifTool/Writer.pl';
local $/; my $rows=JSON::PP->new->utf8->decode(<STDIN>); my @results;
my $cv=\&Image::ExifTool::CheckValue;
my $body=B::Deparse->new('-p','-sC')->coderef2text($cv);
open my $source, '<:raw', B::svref_2object($cv)->FILE or die $!;
my $source_sha=sha256_hex(<$source>); close $source;
for my $r (@$rows) {
    my $value=$r->{kind} eq 'undefined' ? undef :
        $r->{kind} eq 'bytes' ? pack('H*',$r->{value}) : $r->{value};
    utf8::upgrade($value) if $r->{kind} eq 'utf8';
    my $error=Image::ExifTool::CheckValue(\$value,$r->{format},$r->{count});
    my $flag=utf8::is_utf8($value) ? JSON::PP::true : JSON::PP::false;
    my $bytes=defined($value) ? ($flag ? Encode::encode('UTF-8',$value) : $value) : undef;
    push @results, {defined=>defined($value)?JSON::PP::true:JSON::PP::false,
        utf8=>$flag, hex=>defined($bytes)?unpack('H*',$bytes):undef,
        length=>defined($value)?length($value):undef,error=>$error};
}
print JSON::PP->new->canonical->utf8->encode({results=>\@results,
    source_sha256=>$source_sha,body_sha256=>sha256_hex($body)});
'''
        env = os.environ.copy()
        for name in ("PERL5LIB", "PERLLIB", "PERL5OPT"):
            env.pop(name, None)
        result = subprocess.run([env.get("EXIFTOOL_PERL", "/usr/bin/perl"),
                                 "-I" + str(lib), "-e", perl],
                                input=json.dumps(cases, ensure_ascii=False).encode("utf8"),
                                env=env, capture_output=True, timeout=30, check=True)
        native = json.loads(result.stdout)
        self.assertEqual(native["source_sha256"], recipe.provenance.source_sha256)
        self.assertEqual(native["body_sha256"], recipe.provenance.body_sha256)
        self.assertEqual(len(native["results"]), 240)
        self.assertEqual(native["results"], expected)


if __name__ == "__main__":
    unittest.main()
