"""Optional canonical-native proof for ordinary EXIF addressing source gates."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from checkexif_recipes import RecipeRefused
from native_write_matrix import optional_native_configuration
from setnewvalue_addressing import _find_tag_info_source
from setnewvalue_convinv_recipes import compile_setnewvalue_convinv


ROOT = Path(__file__).resolve().parents[2]
_NATIVE_CONFIGURATION = optional_native_configuration(os.environ.get("EXIFTOOL_PERL"), os.environ.get("OXIDEX_PINNED_EXIFTOOL"))
PERL, LIB = _NATIVE_CONFIGURATION if _NATIVE_CONFIGURATION else (None, None)


@unittest.skipUnless(PERL is not None,
                     "requires explicit canonical EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
class NativeSetNewValueAddressingTests(unittest.TestCase):
    def probe_capture(self, lib: Path, *, preload: tuple[str, ...] = ()) -> dict:
        """Small native capture for probe binding tests; no table dump/regen."""
        source = r'''
use strict; use warnings; use B (); use B::Deparse; use Cwd qw(abs_path); use Digest::SHA qw(sha256_hex); use File::Spec; use JSON::PP;
my $lib = abs_path(shift); unshift @INC, $lib; require Image::ExifTool; require 'Image/ExifTool/Writer.pl';
for my $file (@ARGV) { require $file; }
Image::ExifTool::GetTagTable('Image::ExifTool::Exif::Main') or die "no Exif Main\n";
# Match dump_tables.pl: settle the query closure before recording it.
my @settled = Image::ExifTool::TagLookup::FindTagInfo('HostComputer');
sub raw { open(my $f, '<:raw', $_[0]) or die $!; local $/; my $x=<$f>; close($f); return $x }
sub rel { my $a=abs_path($_[0]); die "outside\n" unless index($a, "$lib/")==0; return File::Spec->abs2rel($a,$lib) }
sub helper { my($n)=@_; no strict 'refs'; my $cv=*{$n}{CODE} or die $n; my $b=B::svref_2object($cv); my $g=$b->GV; my $a=($g->STASH->NAME//'').'::'.($g->NAME//''); my $f=rel($b->FILE); my $body=B::Deparse->new('-p','-sC')->coderef2text($cv); return {requested_binding=>$n,actual_name=>$a,source_file=>$f,source_sha256=>sha256_hex(raw("$lib/$f")),body_sha256=>sha256_hex($body)} }
# The v3 probe identifies candidate table pointers against the native catalog.
# Capture that broad load state as well as the selected query state.
my $lookup = raw("$lib/Image/ExifTool/TagLookup.pm");
$lookup =~ /my\s+\@tableList\s*=\s*\(\n(.*?)^\);/ms or die "no table list";
my @tables = ($1 =~ /^\s*'([^']+)'\s*,?\s*(?:#.*)?$/mg);
Image::ExifTool::GetTagTable($_) for @tables;
my @m; for my $i (sort keys %INC) { next unless $i eq 'Image/ExifTool.pm' || $i =~ m{^Image/ExifTool/}; my $f=rel($INC{$i}); push @m,{inc=>$i,source_file=>$f,source_sha256=>sha256_hex(raw($INC{$i}))} }
my $j=JSON::PP->new->canonical->utf8; print $j->encode({native_capture_context=>{schema=>'native_exiftool_capture_context_v1',selected_library=>$lib,perl_path=>(abs_path($^X)//$^X),perl_version=>"$]",exiftool_version=>"$Image::ExifTool::VERSION",loaded_closure=>{sha256=>sha256_hex($j->encode(\@m)),modules=>\@m}},helpers=>{find_tag_info=>helper('Image::ExifTool::TagLookup::FindTagInfo'),set_new_value=>helper('Image::ExifTool::SetNewValue')}});
'''
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / "capture.pl"
            script.write_text(source, encoding="utf-8")
            output = subprocess.run([PERL, str(script), str(lib), *preload], check=True,
                                    text=True, capture_output=True).stdout
        return json.loads(output)

    def perl_body(self, lib: Path, symbol: str, prepend: Path | None = None,
                  *, preload_xmp: bool = False) -> str:
        inc = ([f"-I{prepend}"] if prepend else []) + [f"-I{lib}"]
        modules = ["-MB::Deparse", "-MImage::ExifTool"]
        if preload_xmp:
            modules.append("-MImage::ExifTool::XMP")
        result = subprocess.run(
            [PERL, *inc, *modules, "-e",
             f"require q(Image/ExifTool/Writer.pl); print B::Deparse->new('-p','-sC')->coderef2text(\\&{symbol})"],
            check=True, capture_output=True, text=True)
        return result.stdout

    def code_fact(self, name: str, body: str, source_file: str, source_bytes: bytes, *, requested: str | None = None):
        value = {"__perl": "CODE", "resolved": True, "__name": name, "__deparse": body,
                 "source_file": source_file, "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
                 "dependencies": {}}
        if requested:
            value["requested_binding"] = requested
        return value

    def test_native_case_and_exif_ifd0_selection(self):
        tags = ["HostComputer", "hOsTcOmPuTeR", "EXIF:HostComputer", "IFD0:HostComputer",
                "Software", "IFD0:Software"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tags.json"
            path.write_text(json.dumps(tags))
            result = subprocess.run(
                [PERL, str(ROOT / "tools/exiftool-tables/probe_setnewvalue_addressing_native.pl"), LIB, str(path)],
                check=True, capture_output=True, text=True)
        observed = {item["tag"]: item for item in json.loads(result.stdout)}
        for tag in tags[:4]:
            with self.subTest(tag=tag):
                self.assertEqual(observed[tag]["count"], 1)
                self.assertEqual(observed[tag]["rows"], [{"name": "HostComputer", "raw_id": "316",
                                                           "table_name": "Image::ExifTool::Exif::Main",
                                                           "write_group": "IFD0"}])
        self.assertEqual(observed["Software"]["count"], 8)
        self.assertEqual(observed["IFD0:Software"]["count"], 1)

    def test_copied_lookup_case_and_writer_group_source_changes_refuse(self):
        lib = Path(LIB)
        with tempfile.TemporaryDirectory() as directory:
            copied = Path(directory) / "Image/ExifTool"
            copied.mkdir(parents=True)
            lookup_source = (lib / "Image/ExifTool/TagLookup.pm").read_bytes()
            changed_lookup = lookup_source.replace(b"my $lcTag = lc($tag);", b"my $lcTag = $tag;", 1)
            self.assertNotEqual(changed_lookup, lookup_source)
            (copied / "TagLookup.pm").write_bytes(changed_lookup)
            body = self.perl_body(lib, "Image::ExifTool::TagLookup::FindTagInfo", Path(directory))
            fact = self.code_fact("Image::ExifTool::TagLookup::FindTagInfo", body,
                                  "Image/ExifTool/TagLookup.pm", changed_lookup,
                                  requested="Image::ExifTool::TagLookup::FindTagInfo")
            with self.assertRaisesRegex(RecipeRefused, "FindTagInfo"):
                _find_tag_info_source({"native_write_helpers": {"find_tag_info": fact}})

            writer_source = (lib / "Image/ExifTool/Writer.pl").read_bytes()
            changed_writer = writer_source.replace(
                b"($options{Group}, $tag) = ($1, $2) if $tag =~ /(.*):(.+)/;",
                b"($options{Group}, $tag) = ($2, $1) if $tag =~ /(.*):(.+)/;", 1)
            self.assertNotEqual(changed_writer, writer_source)
            (copied / "Writer.pl").write_bytes(changed_writer)
            setnew_body = self.perl_body(lib, "Image::ExifTool::SetNewValue", Path(directory))
            conv_body = self.perl_body(lib, "Image::ExifTool::ConvInv", Path(directory))
            conv = self.code_fact("Image::ExifTool::ConvInv", conv_body,
                                  "Image/ExifTool/Writer.pl", changed_writer)
            setnew = self.code_fact("Image::ExifTool::SetNewValue", setnew_body,
                                    "Image/ExifTool/Writer.pl", changed_writer,
                                    requested="Image::ExifTool::SetNewValue")
            setnew["dependencies"] = {"Image::ExifTool::ConvInv": conv}
            with self.assertRaisesRegex(RecipeRefused, "caller control flow"):
                compile_setnewvalue_convinv(setnew)

    def test_preloaded_xmp_deparse_spelling_is_closed_and_authenticated(self):
        """The full capture's settled XMP context adds only this one ``&``.

        It is an observed B::Deparse rendering of the same selected source,
        not permission to erase arbitrary ampersands from a source body.
        """
        lib = Path(LIB)
        source = (lib / "Image/ExifTool/TagLookup.pm").read_bytes()
        body = self.perl_body(lib, "Image::ExifTool::TagLookup::FindTagInfo", preload_xmp=True)
        self.assertIn("&Image::ExifTool::XMP::AddFlattenedTags", body)
        fact = self.code_fact("Image::ExifTool::TagLookup::FindTagInfo", body,
                              "Image/ExifTool/TagLookup.pm", source,
                              requested="Image::ExifTool::TagLookup::FindTagInfo")
        _find_tag_info_source({"native_write_helpers": {"find_tag_info": fact}})

        # A second ampersand elsewhere is not a deparser-context spelling of
        # the canonical XMP call and must still reject the whole callable.
        changed = dict(fact)
        changed["__deparse"] = body.replace("GetTagInfoList($table, $tagID)",
                                              "&GetTagInfoList($table, $tagID)", 1)
        self.assertNotEqual(changed["__deparse"], body)
        with self.assertRaisesRegex(RecipeRefused, "FindTagInfo"):
            _find_tag_info_source({"native_write_helpers": {"find_tag_info": changed}})

    def test_probe_replays_authenticated_xmp_closure_before_helper_deparse(self):
        """A full settled capture must be replayed before its helper hash joins."""
        native = self.probe_capture(Path(LIB), preload=("Image/ExifTool/XMP.pm",))
        self.assertIn("Image/ExifTool/XMP.pm", {
            item["inc"] for item in native["native_capture_context"]["loaded_closure"]["modules"]})
        rows = [{"module": "Exif", "table": "Main", "full_name": "Image::ExifTool::Exif::Main",
                 "raw_id": "316", "name": "HostComputer"}]
        canonical = lambda value: json.dumps(value, sort_keys=True, separators=(",", ":"),
                                               ensure_ascii=False).encode()
        capture = {
            "native_capture_context": native["native_capture_context"],
            "find_tag_info": native["helpers"]["find_tag_info"],
            "set_new_value": native["helpers"]["set_new_value"],
            "source_rows_sha256": hashlib.sha256(canonical(rows)).hexdigest(),
            "query_names_sha256": hashlib.sha256(canonical(["hostcomputer"])).hexdigest(),
        }
        with tempfile.TemporaryDirectory() as directory:
            probe_input = Path(directory) / "probe.json"
            probe_input.write_text(json.dumps({"schema": "native_setnewvalue_address_probe_input_v3",
                                                "capture": capture, "rows": rows,
                                                "query_names": ["hostcomputer"]}), encoding="utf-8")
            result = subprocess.run(
                [PERL, str(ROOT / "tools/exiftool-tables/setnewvalue_address_probe.pl"),
                 str(LIB), str(probe_input)], check=False, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
        observed = json.loads(result.stdout)
        self.assertEqual(observed["runtime"]["helpers"]["find_tag_info"], capture["find_tag_info"])
        self.assertEqual(observed["runtime"]["loaded_closure"], capture["native_capture_context"]["loaded_closure"])

    def test_probe_refuses_preloaded_external_exiftool_before_reading_input(self):
        """The selected -I path cannot silently replace an ambient package."""
        with tempfile.TemporaryDirectory() as directory:
            external = Path(directory) / "external/Image"
            external.mkdir(parents=True)
            (external / "ExifTool.pm").write_text(
                "package Image::ExifTool; our $VERSION = 'external'; 1;\n", encoding="utf-8")
            probe_input = Path(directory) / "ignored.json"
            probe_input.write_text("{}", encoding="utf-8")
            result = subprocess.run(
                [PERL, f"-I{Path(directory) / 'external'}", "-MImage::ExifTool",
                 str(ROOT / "tools/exiftool-tables/setnewvalue_address_probe.pl"),
                 str(LIB), str(probe_input)],
                text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("preloaded before selected-library guard", result.stderr)

    def test_probe_refuses_same_path_exif_source_change_after_capture(self):
        """A stable library path cannot stand in for the loaded source bytes."""
        with tempfile.TemporaryDirectory() as directory:
            copied = Path(directory) / "lib"
            shutil.copytree(LIB, copied)
            native = self.probe_capture(copied)
            rows = [{"module": "Exif", "table": "Main", "full_name": "Image::ExifTool::Exif::Main",
                     "raw_id": "316", "name": "HostComputer"}]
            canonical = lambda value: json.dumps(value, sort_keys=True, separators=(",", ":"),
                                                   ensure_ascii=False).encode()
            capture = {
                "native_capture_context": native["native_capture_context"],
                "find_tag_info": native["helpers"]["find_tag_info"],
                "set_new_value": native["helpers"]["set_new_value"],
                "source_rows_sha256": hashlib.sha256(canonical(rows)).hexdigest(),
                "query_names_sha256": hashlib.sha256(canonical(["hostcomputer"])).hexdigest(),
            }
            probe_input = Path(directory) / "probe.json"
            probe_input.write_text(json.dumps({"schema": "native_setnewvalue_address_probe_input_v3",
                                                "capture": capture, "rows": rows,
                                                "query_names": ["hostcomputer"]}), encoding="utf-8")
            exif = copied / "Image/ExifTool/Exif.pm"
            exif.write_bytes(exif.read_bytes() + b"\n# capture binding mutation\n")
            result = subprocess.run(
                [PERL, str(ROOT / "tools/exiftool-tables/setnewvalue_address_probe.pl"),
                 str(copied), str(probe_input)], text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dump capture module source differs from selected library: Image/ExifTool/Exif.pm", result.stderr)


if __name__ == "__main__":
    unittest.main()
