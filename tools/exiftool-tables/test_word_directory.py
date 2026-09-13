"""Closed-grammar and pinned-native checks for the staged word directory."""

import copy
import json
import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
import unittest

import native_reader_contract
import word_directory
from test_native_reader_contract import snapshot


PINNED = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
REPO_ROOT = Path(__file__).resolve().parents[2]
READER_DUMP = REPO_ROOT / "tools" / "exiftool-tables" / "dump_binary_reader_contract.pl"

# This has the complete B::Deparse shape, but no native source dependency.
# It keeps the closed grammar and refusal paths runnable in every Python suite.
FIXTURE_BODY = r'''($$$) {
    package Image::ExifTool::Fixture;
    use strict;
    (my($et, $dirInfo, $tagTablePtr) = @_);
    (my $dataPt = $dirInfo->{'DataPt'});
    (my $offset = $dirInfo->{'DirStart'});
    (my $size = $dirInfo->{'DirLen'});
    (my $verbose = $et->Options('Verbose'));
    (my $len = Get16u($dataPt, $offset));
    unless ((($len == $size) or (($et->{'Model'} =~ /\bWORD\b/) and (($len + 2) == $size)))) {
        $et->Warn('Invalid word data');
        (return 0);
    }
    ($verbose and $et->VerboseDir('Word', (($size / 2) - 1)));
    my($pos);
    for (($pos = 2); ($pos < $size); ($pos += 2)) {
        (my $val = Get16u($dataPt, ($offset + $pos)));
        (my $tag = ($val >> 8));
        ($val = ($val & 255));
        $et->HandleTag($tagTablePtr, $tag, $val, 'Index', (($pos / 2) - 1), 'Format', 'int8u', 'Count', 1, 'Size', 1);
    }
    (return 1);
}'''


def fixture_processor():
    state = snapshot()
    return {
        "__perl": "CODE",
        "__name": "Image::ExifTool::Fixture::Process",
        "resolved": True,
        "__deparse": FIXTURE_BODY,
        "source_file": "Image/ExifTool/Fixture.pm",
        "source_sha256": "a" * 64,
        # Keyed by the bare-call binding in the processor package. The
        # resolved CODE identity remains the core reader.
        "dependencies": {
            "Image::ExifTool::Fixture::Get16u": copy.deepcopy(state["loaded_functions"]["get16u"])
        },
    }


def _capture(root, fallback=None):
    """Capture process and package-local bare-call facts from actual Perl CVs."""
    include = [f"-I{Path(root) / 'lib'}"]
    libraries = [str(Path(root) / "lib")]
    if fallback:
        include.append(f"-I{Path(fallback) / 'lib'}")
        libraries.append(str(Path(fallback) / "lib"))
    script = r'''
use strict;
use B;
use B::Deparse;
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use JSON::PP;
use Image::ExifTool::CanonCustom;
my @lib = map { abs_path($_) } @ARGV;
sub code_name {
    my ($cv) = @_;
    my $gv = B::svref_2object($cv)->GV;
    return $gv->STASH->NAME . '::' . $gv->NAME;
}
sub fact {
    my ($name) = @_;
    no strict 'refs';
    my $cv = *{$name}{CODE} or die "missing CODE $name";
    my $file = abs_path(B::svref_2object($cv)->FILE) or die "missing source $name";
    my $root;
    for my $candidate (@lib) { $root = $candidate if index($file, "$candidate/") == 0; }
    die "outside selected lib $name" unless $root;
    open my $fh, '<:raw', $file or die "read $file";
    local $/; my $bytes = <$fh>;
    return {
        __perl => 'CODE', __name => code_name($cv), resolved => JSON::PP::true,
        __deparse => B::Deparse->new('-p')->coderef2text($cv),
        source_file => substr($file, length($root) + 1), source_sha256 => sha256_hex($bytes),
    };
}
my $process = 'Image::ExifTool::CanonCustom::ProcessCanonCustom';
my $package = 'Image::ExifTool::CanonCustom';
my $record = fact($process);
$record->{dependencies} = { "$package\::Get16u" => fact("$package\::Get16u") };
print JSON::PP->new->canonical->encode($record);
'''
    result = subprocess.run(
        ["/usr/bin/perl", *include, "-MB::Deparse", "-e", script, *libraries],
        check=True, text=True, capture_output=True,
    )
    return json.loads(result.stdout)


def _reader_contract(root):
    result = subprocess.run(
        ["/usr/bin/perl", str(READER_DUMP), str(Path(root) / "lib")],
        check=True, text=True, capture_output=True,
    )
    contract = json.loads(result.stdout)
    # The standalone extractor's successful JSON predates the table dump's
    # explicit top-level marker; the capture wrapper supplies it.
    contract["resolved"] = True
    contract["isolated_functions"] = copy.deepcopy(contract["loaded_functions"])
    contract["isolated_builtin_overrides"] = copy.deepcopy(contract["builtin_overrides"])
    return contract


class ClosedGrammar(unittest.TestCase):
    def setUp(self):
        self.reader = snapshot()
        self.processor = fixture_processor()

    def compile(self, record=None, reader=None):
        return word_directory.compile_word_directory(
            record or self.processor, {"unsigned16": reader or self.reader}
        )

    def test_fixture_compiles_staged_operands_and_package_binding(self):
        compiled = self.compile()
        self.assertEqual(
            (compiled.pair_start, compiled.pair_stride, compiled.key_shift, compiled.value_mask,
             compiled.header_adjustment, compiled.index_divisor, compiled.index_bias),
            (2, 2, 8, 255, 2, 2, 1),
        )
        self.assertEqual((compiled.value_format, compiled.value_count, compiled.value_size), ("int8u", 1, 1))
        self.assertIn("MemberRegex", compiled.model_condition)
        self.assertTrue(compiled.exact_length_first)
        self.assertTrue(compiled.missing_model_as_empty)
        self.assertTrue(compiled.short_u16_as_zero)
        self.assertEqual(compiled.reader_contract_sha256, native_reader_contract.fingerprint(self.reader))
        staged = compiled.rust(lambda value: value.replace('"', '\\"'))
        self.assertIn("key_shift: 8", staged)
        self.assertIn("value_mask: 255", staged)
        self.assertIn("value_format: \"int8u\"", staged)

    def test_refuses_missing_or_rebound_package_binding(self):
        missing = copy.deepcopy(self.processor)
        missing["dependencies"] = {"Image::ExifTool::Get16u": copy.deepcopy(self.reader["loaded_functions"]["get16u"])}
        with self.assertRaises(word_directory.WordDirectoryRefused):
            self.compile(missing)
        rebound = copy.deepcopy(self.processor)
        rebound["dependencies"]["Image::ExifTool::Fixture::Get16u"]["__name"] = "Image::ExifTool::Get32u"
        with self.assertRaises(word_directory.WordDirectoryRefused):
            self.compile(rebound)

    def test_refuses_dynamic_regex_and_unmodeled_handler_semantics(self):
        cases = [
            ("dynamic regex", "/\\bWORD\\b/", "/$size/"),
            ("value format", "'int8u'", "'int16u'"),
            ("extra side effect", "(return 1);", "($et->Warn('extra'); return 1);"),
        ]
        for label, before, after in cases:
            changed = copy.deepcopy(self.processor)
            changed["__deparse"] = changed["__deparse"].replace(before, after, 1)
            with self.subTest(label=label), self.assertRaises(word_directory.WordDirectoryRefused):
                self.compile(changed)
        escaped = copy.deepcopy(self.processor)
        escaped["__deparse"] = escaped["__deparse"].replace("/\\bWORD\\b/", "/\\$size/", 1)
        self.assertIsNotNone(self.compile(escaped))

    def test_diagnostic_literals_are_decoded_and_dynamic_or_perl_only_forms_refuse(self):
        escaped = copy.deepcopy(self.processor)
        escaped["__deparse"] = escaped["__deparse"].replace(
            "'Invalid word data'", '"Invalid\\nword"', 1
        )
        self.assertEqual(self.compile(escaped).invalid_warning, "Invalid\nword")
        for label, before, after in (
            ("dynamic warning", "'Invalid word data'", '"Invalid $size"'),
            ("dynamic verbose", "'Word'", '"Word $size"'),
            ("Perl-only escape", "'Invalid word data'", '"Invalid\\x{41}"'),
        ):
            changed = copy.deepcopy(self.processor)
            changed["__deparse"] = changed["__deparse"].replace(before, after, 1)
            with self.subTest(label=label), self.assertRaises(word_directory.WordDirectoryRefused):
                self.compile(changed)

    def test_stale_provenance_and_reader_contract_do_not_reuse_descriptor(self):
        baseline = self.compile()
        changed = copy.deepcopy(self.processor)
        changed["source_sha256"] = "3" * 64
        self.assertEqual(word_directory.stale_reason(
            baseline, changed, {"unsigned16": self.reader}
        ), "native processor operands or provenance differ")
        changed_reader = snapshot()
        for where in (changed_reader["loaded_functions"], changed_reader["isolated_functions"]):
            where["get16u"]["__deparse"] = where["get16u"]["__deparse"].replace("'S'", "'L'")
        with self.assertRaises(word_directory.WordDirectoryRefused):
            self.compile(reader=changed_reader)


@unittest.skipUnless(PINNED, "set OXIDEX_PINNED_EXIFTOOL for native capture checks")
class PinnedNative(unittest.TestCase):
    def setUp(self):
        self.reader = _reader_contract(PINNED)
        self.processor = _capture(PINNED)

    def compile(self, record=None):
        return word_directory.compile_word_directory(record or self.processor, {"unsigned16": self.reader})

    def copied_source_processor(self, before, after, *, whole_file=False):
        """Mutate copied native source, then recapture its actual CODE facts."""
        tmp = TemporaryDirectory()
        target = Path(tmp.name) / "lib" / "Image" / "ExifTool"
        target.mkdir(parents=True)
        source = Path(PINNED) / "lib" / "Image" / "ExifTool" / "CanonCustom.pm"
        text = source.read_text()
        if whole_file:
            self.assertIn(before, text)
            changed = text.replace(before, after, 1)
        else:
            marker = "sub ProcessCanonCustom($$$)\n{"
            prefix, process = text.split(marker, 1)
            self.assertIn(before, process)
            changed = prefix + marker + process.replace(before, after, 1)
        (target / "CanonCustom.pm").write_text(changed)
        self.addCleanup(tmp.cleanup)
        return _capture(tmp.name, fallback=PINNED)

    def test_captured_native_binding_is_package_keyed_and_compiles(self):
        binding = "Image::ExifTool::CanonCustom::Get16u"
        self.assertIn(binding, self.processor["dependencies"])
        self.assertEqual(self.processor["dependencies"][binding]["__name"], "Image::ExifTool::Get16u")
        compiled = self.compile()
        self.assertEqual((compiled.pair_start, compiled.pair_stride, compiled.key_shift, compiled.value_mask),
                         (2, 2, 8, 255))

    def test_source_mutations_change_operands_or_refuse(self):
        baseline = self.compile()
        cases = [
            ("model predicate", "\\bD60\\b", "\\bD61\\b", "changed"),
            ("key shift", "$val >> 8", "$val >> 7", "changed"),
            ("value mask", "$val & 0xff", "$val & 0x7f", "changed"),
            ("dynamic regex", "$$et{Model}=~/\\bD60\\b/", "$$et{Model}=~/$size/", "refused"),
            ("value format", "Format => 'int8u'", "Format => 'int16u'", "refused"),
        ]
        for label, before, after, expectation in cases:
            changed = self.copied_source_processor(before, after)
            with self.subTest(label=label):
                if expectation == "changed":
                    fresh = self.compile(changed)
                    self.assertNotEqual(fresh.fingerprint(), baseline.fingerprint())
                    self.assertEqual(word_directory.stale_reason(baseline, changed, {"unsigned16": self.reader}),
                                     "native processor operands or provenance differ")
                else:
                    with self.assertRaises(word_directory.WordDirectoryRefused):
                        self.compile(changed)

    def test_real_package_rebinding_refuses_unchanged_processor_body(self):
        changed = self.copied_source_processor(
            "use Image::ExifTool::Exif;",
            "use Image::ExifTool::Exif;\n*Image::ExifTool::CanonCustom::Get16u = \\&Image::ExifTool::Get32u;",
            whole_file=True,
        )
        self.assertEqual(changed["__deparse"], self.processor["__deparse"])
        binding = "Image::ExifTool::CanonCustom::Get16u"
        self.assertEqual(changed["dependencies"][binding]["__name"], "Image::ExifTool::Get32u")
        with self.assertRaises(word_directory.WordDirectoryRefused):
            self.compile(changed)


if __name__ == "__main__":
    unittest.main()
