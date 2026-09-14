"""Native write-fact sidecar coverage.

The sidecar is source evidence only.  These tests prove it preserves the
write-relevant native values and effective procedure provenance without
changing the existing read-table projection or admitting any writer.
"""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

import write_descriptors
from sanitize_recipes import _CANONICAL as SANITIZE_BODY, compile_sanitize
from checkexif_recipes import RecipeRefused
from setnewvalue_convinv_recipes import compile_setnewvalue_convinv


REPO_ROOT = Path(__file__).resolve().parents[2]
DUMP = REPO_ROOT / "tools/exiftool-tables/dump_tables.pl"
PERL = os.environ.get("EXIFTOOL_PERL", "/usr/bin/perl")


class NativeWriteFacts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = Path(self.tmp.name) / "lib"
        package = self.lib / "Image/ExifTool"
        package.mkdir(parents=True)
        (self.lib / "Image/ExifTool.pm").write_text(
            textwrap.dedent("""\
                package Image::ExifTool;
                use strict;
                our $VERSION = 'fixture';
                our %specialTags = map { $_ => 1 }
                    qw(GROUPS WRITE_PROC CHECK_PROC WRITABLE WRITE_GROUP FORMAT FIRST_ENTRY);
                sub DoAutoLoad(@) {
                    my ($autoload) = shift;
                    my @callInfo = split /::/, $autoload, 0;
                    my ($file) = 'Image/ExifTool/Write';
                    ($callInfo[$#callInfo] eq 'DESTROY') and return;
                    if (@callInfo == 4) {
                        $file .= "$callInfo[2].pl";
                    } elsif ($callInfo[-1] eq 'ShiftTime') {
                        $file = 'Image/ExifTool/Shift.pl';
                    } else {
                        $file .= 'r.pl';
                    }
                    eval { require $file }
                        or die("Error while attempting to call $autoload\n$@\n");
                    unless (defined &$autoload) {
                        my @caller = caller(0);
                        die("Undefined subroutine $autoload called at $caller[1] line $caller[2]\n");
                    }
                    no strict 'refs';
                    return &$autoload(@_);
                }
                sub new { return bless {}, shift; }
                sub GetTagInfo($$$) {
                    my ($self, $table, $id) = @_;
                    die "condition lookup was evaluated\n" if
                        ref($table->{$id}) eq 'HASH' and exists $table->{$id}{Condition};
                    if ($Image::ExifTool::Exif::RedirectEffective and $id == 316) {
                        my %redirected = %{ $table->{$id} };
                        $redirected{Table} = \\%Image::ExifTool::Exif::Redirected;
                        return \\%redirected;
                    }
                    return $table->{$id};
                }
                1;
            """),
            encoding="utf-8",
        )
        self.exif = package / "Exif.pm"
        self.writer = package / "WriteExif.pl"
        self.writer_helpers = package / "Writer.pl"
        self.tag_lookup = package / "TagLookup.pm"
        self.later = package / "Later.pm"
        self.write_exif("IFD0")
        self.write_writer("return 1;")
        self.write_tag_lookup()
        self.write_writer_helpers()
        self.write_later("return 'late';")

    def write_exif(self, host_group: str, host_group_expr: str | None = None,
                   host_condition: str | None = None, *, redirect_effective: bool = False,
                   host_source_extra: str = ""):
        write_group = host_group_expr if host_group_expr is not None else f"'{host_group}'"
        condition = f", Condition => {host_condition}" if host_condition is not None else ""
        self.exif.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::Exif;
            sub WriteExif($$$);
            sub CheckExif($$$);
            sub DeferredWrite($$$);
            our $RedirectEffective = {1 if redirect_effective else 0};
            our %Redirected = ( CHECK_PROC => \\&DeferredWrite );
            our %Main = (
                GROUPS => {{ 0 => 'EXIF', 1 => 'IFD0', 2 => 'Image' }},
                WRITE_PROC => \\&WriteExif,
                CHECK_PROC => \\&CheckExif,
                WRITE_GROUP => 'ExifIFD',
                WRITABLE => 0,
                0x13c => {{
                    Name => 'HostComputer', Writable => 'string',
                    WriteGroup => {write_group}, RawConvInv => '$val',
                    ValueConvInv => 0, PrintConvInv => '', Mandatory => 0,
                    DelValue => '', Validate => undef,
                    FutureWriterSwitch => {{ zero => 0, empty => '', undef => undef }}{condition}{host_source_extra},
                }},
                0x140 => [
                    {{ Name => 'First', WriteLast => 0, WriteGroup => 'IFD0' }},
                    {{ Name => 'Second', WriteLast => 1, WriteGroup => undef }},
                ],
            );
            our %Deferred = (
                WRITE_PROC => \\&DeferredWrite,
                1 => {{ Name => 'DeferredTag', Writable => 1 }},
            );
            1;
        """), encoding="utf-8")

    def write_writer(self, body: str, *, extra: str = ""):
        self.writer.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::Exif;
            require 'Image/ExifTool/Writer.pl';
            sub WriteExif($$$) {{ {body} }}
            sub CheckExif($$$) {{ return undef; }}
            {extra}
            1;
        """), encoding="utf-8")

    def write_writer_helpers(self, *, write_body="return Image::ExifTool::WriterHelper($_[0]);",
                             check_body="return Image::ExifTool::CheckHelper($_[0]);",
                             sanitize_body="return Encode::encode('utf8', $_[1]);",
                             set_new_value_body="return 1;"):
        self.writer_helpers.write_text(textwrap.dedent(f"""\
            package Image::ExifTool;
            require 'Image/ExifTool/TagLookup.pm';
            sub WriterHelper($) {{ return $_[0]; }}
            sub CheckHelper($) {{ return $_[0]; }}
            sub WriteValue($$) {{ {write_body} }}
            sub CheckValue($$) {{ {check_body} }}
            sub Sanitize($$) {{ {sanitize_body} }}
            sub ConvInv($$$$$$) {{ return; }}
            sub SetNewValue($;$$%) {{ {set_new_value_body} }}
            1;
        """), encoding="utf-8")

    def write_tag_lookup(self, *, body="return;"):
        self.tag_lookup.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::TagLookup;
            sub FindTagInfo($) {{ {body} }}
            1;
        """), encoding="utf-8")

    def write_later(self, body: str, *, extra: str = ""):
        # This module is loaded after Exif.  It fills the prototype stored by
        # `%Deferred`, so the final source binding must describe this file.
        self.later.write_text(textwrap.dedent(f"""\
            package Image::ExifTool::Exif;
            sub DeferredWrite($$$) {{ {body} }}
            {extra}
            1;
        """), encoding="utf-8")

    def dump(self, extra_env: dict[str, str] | None = None):
        env = os.environ.copy()
        if extra_env:
            env.update(extra_env)
        result = subprocess.run(
            [PERL, str(DUMP), str(self.lib), "Exif", "Later"],
            check=True, text=True, capture_output=True, env=env,
        )
        return json.loads(result.stdout)

    @staticmethod
    def sidecar(doc, table="Main"):
        return doc["native_write_tables"]["Exif"][table]

    def test_writer_capture_context_loads_selected_modules_without_erasing_prototype_calls(self):
        self.write_writer("return Image::ExifTool::Canon::ReadODD(@_);")
        before = self.dump()
        self.assertFalse(before["native_write_capture_context"]["resolved"])
        for name, body in (("Canon", "sub ReadODD($) { return join ',', @_; }"),
                           ("PanasonicRaw", ""), ("Sony", "")):
            (self.lib / f"Image/ExifTool/{name}.pm").write_text(f"package Image::ExifTool::{name}; {body} 1;\n")
        after = self.dump()
        context = after["native_write_capture_context"]
        self.assertTrue(context["resolved"])
        self.assertEqual(context["deparse_options"], ["-p", "-sC"])
        for item in context["modules"]:
            self.assertEqual(item["source_sha256"], hashlib.sha256((self.lib / item["file"]).read_bytes()).hexdigest())
        body = self.sidecar(after)["effective_write_proc"]["effective"]["__deparse"]
        self.assertIn("&Image::ExifTool::Canon::ReadODD(@_)", body)
        self.assertNotIn("&Image::ExifTool::Canon::ReadODD", self.sidecar(before)["effective_write_proc"]["effective"]["__deparse"])
        self.assertEqual(before["modules"], after["modules"])
        self.assertEqual(after, self.dump({"OXIDEX_DISABLE_CODE_FACT_CACHE": "1"}))

    def test_writer_context_refuses_ambient_transitive_native_module(self):
        for name in ("Canon", "PanasonicRaw", "Sony"):
            dependency = "require 'Image/ExifTool/Minolta.pm';" if name == "Sony" else ""
            (self.lib / f"Image/ExifTool/{name}.pm").write_text(f"package Image::ExifTool::{name}; {dependency} 1;\n")
        ambient = Path(self.tmp.name) / "ambient"
        minolta = ambient / "Image/ExifTool/Minolta.pm"
        minolta.parent.mkdir(parents=True)
        minolta.write_text("package Image::ExifTool::Minolta; 1;\n")
        context = self.dump({"PERL5LIB": str(ambient)})["native_write_capture_context"]
        self.assertFalse(context["resolved"])
        self.assertTrue(all(item["resolved"] for item in context["modules"]))
        self.assertEqual(context["load_errors"], [{"file": "Image/ExifTool/Minolta.pm", "reason": "loaded_context_module_outside_selected_library"}])

    def test_preserves_complete_controls_variants_and_unknown_values(self):
        doc = self.dump()
        table = self.sidecar(doc)
        host = table["rows"]["316"]
        controls = host["write_controls"]
        self.assertEqual(controls["Writable"], {"present": True, "value": "string"})
        self.assertEqual(controls["WriteGroup"], {"present": True, "value": "IFD0"})
        # Perl table scalars have no numeric/string type bit.  The source
        # spelling survives as the distinct non-empty scalar "0", which is
        # what matters against absence, undef, and the empty scalar below.
        self.assertEqual(controls["Mandatory"], {"present": True, "value": "0"})
        self.assertEqual(controls["DelValue"], {"present": True, "value": ""})
        self.assertEqual(controls["Validate"], {"present": True, "value": None})
        self.assertFalse(controls["WriteAlso"]["present"])
        self.assertEqual(host["properties"]["RawConvInv"]["classification"],
                         {"kind": "expr", "expr": "$val"})
        self.assertEqual(host["unknown_properties"]["FutureWriterSwitch"]["value"],
                         {"empty": "", "undef": None, "zero": "0"})

        variants = table["rows"]["320"]["alternatives"]
        self.assertEqual([item["properties"]["Name"]["value"] for item in variants],
                         ["First", "Second"])
        self.assertEqual(variants[0]["write_controls"]["WriteLast"]["value"], "0")
        self.assertEqual(variants[1]["write_controls"]["WriteGroup"],
                         {"present": True, "value": None})
        # Table defaults and row overrides remain separate source facts.
        self.assertEqual(table["write_controls"]["WRITE_GROUP"],
                         {"present": True, "value": "ExifIFD"})

    def test_scalar_reference_is_not_collapsed_to_literal(self):
        self.write_exif("unused", host_group_expr="\\'IFD0'")
        value = self.sidecar(self.dump())["rows"]["316"]["write_controls"]["WriteGroup"]["value"]
        self.assertEqual(value, {"__ref": "SCALAR", "value": "IFD0"})

    def test_condition_row_is_never_resolved_in_context_free_effective_lookup(self):
        self.write_exif("IFD0", host_condition="'die q(condition lookup was evaluated)'")
        host = self.sidecar(self.dump())["rows"]["316"]
        self.assertEqual(host["effective_resolution"], "native_get_tag_info_condition_unrepresented")
        self.assertEqual(host["properties"]["Condition"]["value"],
                         "die q(condition lookup was evaluated)")

    def test_redirected_effective_table_is_recorded_not_treated_as_containing(self):
        self.write_exif("IFD0", redirect_effective=True)
        host = self.sidecar(self.dump())["rows"]["316"]
        self.assertEqual(host["effective_resolution"], "native_get_tag_info")
        self.assertEqual(host["effective_table_binding"], {
            "kind": "redirected_table", "ref_identical_to_containing": False,
        })

    def test_unusual_raw_table_and_tagid_properties_are_preserved(self):
        self.write_exif("IFD0", host_source_extra=", Table => { source => 'unusual' }, TagID => 'raw-id'")
        host = self.sidecar(self.dump())["rows"]["316"]
        self.assertEqual(host["properties"]["Table"], {
            "present": True,
            "value": {"__ref": "HASH", "table_ref_identical_to_containing": False},
        })
        self.assertEqual(host["properties"]["TagID"], {
            "present": True, "value": "raw-id",
        })
        self.assertIn("Table", host["unknown_properties"])
        self.assertIn("TagID", host["unknown_properties"])

    def test_mutated_autoload_router_refuses_forced_writer_loading(self):
        core = self.lib / "Image/ExifTool.pm"
        text = core.read_text(encoding="utf-8")
        for label, changed in {
            "changed_route": text.replace("Image/ExifTool/Write", "Image/ExifTool/Broken", 1),
            "injected_route": text.replace(
                "eval { require $file }", "$file = 'Image/ExifTool/WriteNoSuchWriter.pl';\n                    eval { require $file }"),
            "early_return": text.replace("eval { require $file }", "return;\n                    eval { require $file }"),
        }.items():
            with self.subTest(label=label):
                core.write_text(changed, encoding="utf-8")
                doc = self.dump()
                router = doc["native_write_autoload"]
                self.assertFalse(router["supported"])
                effective = self.sidecar(doc)["effective_write_proc"]["effective"]
                self.assertFalse(effective["resolved"])
                self.assertEqual(effective["reason"], "autoload_router_unsupported")
        core.write_text(text, encoding="utf-8")

    def test_forward_declarations_capture_loaded_writer_bodies(self):
        doc = self.dump()
        self.assertEqual(doc, self.dump({"OXIDEX_DISABLE_CODE_FACT_CACHE": "1"}))
        table = self.sidecar(doc)
        for key in ("effective_write_proc", "effective_check_proc"):
            effective = table[key]["effective"]
            self.assertTrue(effective["resolved"])
            self.assertEqual(effective["source_file"], "Image/ExifTool/WriteExif.pl")
            self.assertEqual(effective["source_sha256"],
                             hashlib.sha256(self.writer.read_bytes()).hexdigest())
            self.assertNotEqual(effective["__deparse"].strip(), "($$$) ;")
        deferred = self.sidecar(doc, "Deferred")["effective_write_proc"]["effective"]
        self.assertTrue(deferred["resolved"])
        self.assertEqual(deferred["source_file"], "Image/ExifTool/Later.pm")
        self.assertEqual(deferred["source_sha256"],
                         hashlib.sha256(self.later.read_bytes()).hexdigest())

    def test_captures_final_write_value_and_check_value_bindings_with_dependencies(self):
        helpers = self.dump()["native_write_helpers"]
        for key, callable_name, dependency in (
            ("write_value", "Image::ExifTool::WriteValue", "Image::ExifTool::WriterHelper"),
            ("check_value", "Image::ExifTool::CheckValue", "Image::ExifTool::CheckHelper"),
        ):
            with self.subTest(key=key):
                fact = helpers[key]
                self.assertTrue(fact["resolved"])
                self.assertEqual(fact["requested_binding"], callable_name)
                self.assertEqual(fact["__name"], callable_name)
                self.assertEqual(fact["source_file"], "Image/ExifTool/Writer.pl")
                self.assertEqual(fact["source_sha256"],
                                 hashlib.sha256(self.writer_helpers.read_bytes()).hexdigest())
                self.assertIn(dependency, fact["dependencies"])
                self.assertTrue(fact["dependencies"][dependency]["resolved"])

    def test_sanitize_keeps_source_and_unresolved_external_dependency(self):
        fact = self.dump()["native_write_helpers"]["sanitize"]
        self.assertTrue(fact["resolved"])
        self.assertEqual(fact["requested_binding"], "Image::ExifTool::Sanitize")
        self.assertEqual(fact["source_file"], "Image/ExifTool/Writer.pl")
        self.assertEqual(fact["source_sha256"],
                         hashlib.sha256(self.writer_helpers.read_bytes()).hexdigest())
        # Encode is supplied by Perl, outside the selected ExifTool library.
        # Retain that unresolved dependency rather than inventing its source
        # or turning a captured helper body into executable admission.
        dependency = fact["dependencies"]["Encode::encode"]
        self.assertFalse(dependency["resolved"])
        self.assertIsNone(dependency["source_sha256"])
        self.assertIn(dependency["reason"], {
            "source_file_unreadable", "source_outside_selected_lib",
            "deparse_unavailable",
        })

    def test_writer_constructor_mutation_preserves_read_projection(self):
        main = self.lib / "Image/ExifTool.pm"
        source = main.read_text()
        original = "sub new { return bless {}, shift; }"
        self.assertIn(original, source)
        main.write_text(source.replace(original, """sub new {
            require Image::ExifTool::Exif;
            $Image::ExifTool::Exif::Main{316}{Name} = 'ConstructorMutated';
            return bless {}, shift;
        }"""))
        document = self.dump()
        self.assertEqual(document["modules"]["Exif"]["tables"]["Main"]["tags"]["316"]["Name"],
                         "HostComputer")
        self.assertEqual(self.sidecar(document)["rows"]["316"]["properties"]["Name"]["value"],
                         "ConstructorMutated")

    def test_sanitize_source_change_preserves_read_projection(self):
        before = self.dump()
        self.write_writer_helpers(sanitize_body="return Encode::encode('UTF-16', $_[1]);")
        after = self.dump()
        self.assertEqual(before["modules"], after["modules"])
        old = before["native_write_helpers"]["sanitize"]
        new = after["native_write_helpers"]["sanitize"]
        self.assertNotEqual(old["__deparse"], new["__deparse"])
        self.assertNotEqual(old["source_sha256"], new["source_sha256"])
        self.assertIn("UTF-16", new["__deparse"])

    def test_sanitize_rebinding_records_actual_callable(self):
        self.write_writer("return 1;", extra=textwrap.dedent("""\
            package Image::ExifTool;
            no warnings 'redefine';
            *Image::ExifTool::Sanitize = sub($$) { return 'replacement'; };
        """))
        fact = self.dump()["native_write_helpers"]["sanitize"]
        self.assertEqual(fact["requested_binding"], "Image::ExifTool::Sanitize")
        self.assertNotEqual(fact["__name"], fact["requested_binding"])
        self.assertEqual(fact["source_file"], "Image/ExifTool/WriteExif.pl")
        self.assertIn("replacement", fact["__deparse"])

    def test_sanitize_callback_reference_tracks_final_binding(self):
        self.write_writer_helpers(sanitize_body=(
            r"local $SIG{'__WARN__'} = \&Image::ExifTool::WarningHandler; return $_[1];"))
        before = self.dump()["native_write_helpers"]["sanitize"]
        name = "Image::ExifTool::WarningHandler"
        self.assertFalse(before["callback_references"][name]["resolved"])
        self.write_later("return 'late';", extra=textwrap.dedent("""\
            package Image::ExifTool;
            sub WarningHandler { return 'loaded callback'; }
        """))
        after = self.dump()["native_write_helpers"]["sanitize"]
        reference = after["callback_references"][name]
        self.assertTrue(reference["resolved"])
        self.assertEqual(reference["source_file"], "Image/ExifTool/Later.pm")
        self.assertEqual(reference["source_sha256"],
                         hashlib.sha256(self.later.read_bytes()).hexdigest())
        self.assertIn("loaded callback", reference["__deparse"])

    def test_official_capture_compiles_source_changed_sanitize_guards(self):
        # Real Perl loads and B::Deparse captures this synthetic helper through
        # the production dumper. This tests the capture/compiler join, not a
        # claim about native ExifTool or a generated runtime.
        # Deparse inserts a do block inside eval; remove that rendering layer
        # when using its text as fixture source, rather than weakening parsing.
        source = SANITIZE_BODY.replace("eval {\ndo {", "eval {").replace(
            "Encode::is_utf8($$valPt)\n}\n}", "Encode::is_utf8($$valPt)\n}")
        for guard, expected in (("5.006", 5_006_000), ("10.006", 10_006_000)):
            self.writer_helpers.write_text(
                "package Image::ExifTool;\nsub Sanitize" +
                source.replace("5.006", guard) + "\n1;\n",
                encoding="utf-8")
            fact = self.dump()["native_write_helpers"]["sanitize"]
            recipe = compile_sanitize(fact)
            self.assertEqual(recipe.downgrade_at_or_after, expected)
            self.assertEqual(recipe.provenance.source_sha256,
                             hashlib.sha256(self.writer_helpers.read_bytes()).hexdigest())
            self.assertIn("Image::ExifTool::SetWarning", recipe.callback_references)

    def test_captures_final_setnewvalue_binding(self):
        fact = self.dump()["native_write_helpers"]["set_new_value"]
        self.assertTrue(fact["resolved"])
        self.assertEqual(fact["requested_binding"], "Image::ExifTool::SetNewValue")
        self.assertEqual(fact["__name"], "Image::ExifTool::SetNewValue")
        self.assertEqual(fact["source_file"], "Image/ExifTool/Writer.pl")
        self.assertEqual(fact["source_sha256"], hashlib.sha256(self.writer_helpers.read_bytes()).hexdigest())

    def test_captures_final_findtaginfo_binding(self):
        fact = self.dump()["native_write_helpers"]["find_tag_info"]
        self.assertTrue(fact["resolved"])
        self.assertEqual(fact["requested_binding"], "Image::ExifTool::TagLookup::FindTagInfo")
        self.assertEqual(fact["__name"], "Image::ExifTool::TagLookup::FindTagInfo")
        self.assertEqual(fact["source_file"], "Image/ExifTool/TagLookup.pm")
        self.assertEqual(fact["source_sha256"], hashlib.sha256(self.tag_lookup.read_bytes()).hexdigest())

    def test_setnewvalue_defined_false_source_mutation_is_captured_and_refused(self):
        self.write_writer_helpers(set_new_value_body=textwrap.dedent("""\
            my ($val, $e) = $self->ConvInv($val, $tagInfo, $tag, $wgrp1, $convType, $wantGroup);
            if (defined $e) { $e or return; }
            return;
        """))
        before = self.dump()["native_write_helpers"]["set_new_value"]
        self.write_writer_helpers(set_new_value_body=textwrap.dedent("""\
            my ($val, $e) = $self->ConvInv($val, $tagInfo, $tag, $wgrp1, $convType, $wantGroup);
            if ($e) { return; }
            return;
        """))
        after = self.dump()["native_write_helpers"]["set_new_value"]
        self.assertNotEqual(before["source_sha256"], after["source_sha256"])
        self.assertNotEqual(before["__deparse"], after["__deparse"])
        with self.assertRaisesRegex(RecipeRefused, "caller control flow"):
            compile_setnewvalue_convinv(after)

    def test_helper_rebinding_and_body_mutation_are_captured_after_writer_autoload(self):
        before = self.dump()
        self.write_writer("return 1;", extra=textwrap.dedent("""\
            package Image::ExifTool;
            no warnings 'redefine';
            *Image::ExifTool::WriteValue = sub($$) { return 'later'; };
        """))
        rebound = self.dump()
        fact = rebound["native_write_helpers"]["write_value"]
        self.assertTrue(fact["resolved"])
        self.assertEqual(fact["requested_binding"], "Image::ExifTool::WriteValue")
        self.assertNotEqual(fact["__name"], fact["requested_binding"])
        self.assertEqual(fact["source_file"], "Image/ExifTool/WriteExif.pl")
        self.assertNotEqual(before["native_write_helpers"]["write_value"]["__deparse"], fact["__deparse"])

        self.write_later("return 'late';")
        self.write_writer_helpers(write_body="return Image::ExifTool::WriterHelper('changed');")
        changed = self.dump()
        self.assertNotEqual(
            before["native_write_helpers"]["write_value"]["source_sha256"],
            changed["native_write_helpers"]["write_value"]["source_sha256"],
        )
        self.assertEqual(before["modules"], changed["modules"])
        before_candidates, _ = write_descriptors.generate(before, ["Exif"])
        changed_candidates, _ = write_descriptors.generate(changed, ["Exif"])
        self.assertEqual(before_candidates, changed_candidates)

    def test_memoized_code_facts_match_uncached_after_final_rebinding(self):
        uncached = {"OXIDEX_DISABLE_CODE_FACT_CACHE": "1"}
        before = self.dump()
        self.assertEqual(before, self.dump(uncached))
        self.write_writer("return 1;", extra=textwrap.dedent("""\
            package Image::ExifTool;
            no warnings 'redefine';
            *Image::ExifTool::WriteValue = sub($$) { return 'later'; };
        """))
        rebound = self.dump()
        self.assertEqual(rebound, self.dump(uncached))
        self.assertNotEqual(before["native_write_helpers"]["write_value"]["__deparse"],
                            rebound["native_write_helpers"]["write_value"]["__deparse"])

    def test_missing_helper_is_explicit_without_discarding_the_read_projection(self):
        self.writer_helpers.write_text(textwrap.dedent("""\
            package Image::ExifTool;
            sub WriteValue($$) { return $_[0]; }
            1;
        """), encoding="utf-8")
        doc = self.dump()
        self.assertTrue(doc["native_write_helpers"]["write_value"]["resolved"])
        missing = doc["native_write_helpers"]["check_value"]
        self.assertFalse(missing["resolved"])
        self.assertEqual(missing["requested_binding"], "Image::ExifTool::CheckValue")
        self.assertEqual(missing["reason"], "code_ref_unavailable")
        self.assertFalse(missing["lexical_hashes"]["resolved"])
        missing_sanitize = doc["native_write_helpers"]["sanitize"]
        self.assertFalse(missing_sanitize["resolved"])
        self.assertEqual(missing_sanitize["requested_binding"], "Image::ExifTool::Sanitize")
        self.assertEqual(missing_sanitize["reason"], "code_ref_unavailable")
        self.assertIn("Exif", doc["modules"])

    def test_loaded_lexical_dispatch_changes_without_changing_helper_body(self):
        source = textwrap.dedent("""\
            package Image::ExifTool;
            sub WriterHelper($) { return $_[0]; }
            my %formats = (
                int8u => \\&WriterHelper,
                string => sub($) { return 'intercepted'; },
                empty => '', zero => 0, missing => undef, range => [-2, 9],
            );
            # MUTATE_LOADED_MAP
            sub WriteValue($$) {
                my ($value, $format) = @_;
                return $formats{$format};
            }
            sub CheckValue($$) { return undef; }
            1;
        """)
        self.writer_helpers.write_text(source.replace(
            "# MUTATE_LOADED_MAP", "delete $formats{string};"
        ), encoding="utf-8")
        before = self.dump()
        self.writer_helpers.write_text(source, encoding="utf-8")
        after = self.dump()
        old = before["native_write_helpers"]["write_value"]
        new = after["native_write_helpers"]["write_value"]
        self.assertEqual(old["__deparse"], new["__deparse"])
        self.assertEqual(before["modules"], after["modules"])
        self.assertTrue(new["lexical_hashes"]["resolved"])
        old_map = old["lexical_hashes"]["bindings"]["%formats"]
        new_map = new["lexical_hashes"]["bindings"]["%formats"]
        self.assertTrue(old_map["resolved"] and new_map["resolved"])
        self.assertNotIn("string", old_map["entries"])
        entries = new_map["entries"]
        self.assertTrue(entries["string"]["resolved"])
        self.assertIn("__ANON__", entries["string"]["__name"])
        self.assertEqual(entries["string"]["source_sha256"],
                         hashlib.sha256(self.writer_helpers.read_bytes()).hexdigest())
        self.assertEqual(entries["int8u"]["__name"], "Image::ExifTool::WriterHelper")
        self.assertEqual(entries["empty"], "")
        self.assertEqual(str(entries["zero"]), "0")
        self.assertIsNone(entries["missing"])
        self.assertEqual([str(value) for value in entries["range"]], ["-2", "9"])

    def test_lexical_capture_uses_final_rebound_callable_and_preserves_empty_map(self):
        self.write_writer("return 1;", extra=textwrap.dedent("""\
            package Image::ExifTool;
            my %replacement;
            no warnings 'redefine';
            *Image::ExifTool::WriteValue = sub($$) {
                return $replacement{$_[1]};
            };
        """))
        fact = self.dump()["native_write_helpers"]["write_value"]
        self.assertEqual(fact["requested_binding"], "Image::ExifTool::WriteValue")
        self.assertEqual(fact["source_file"], "Image/ExifTool/WriteExif.pl")
        self.assertEqual(fact["lexical_hashes"], {
            "resolved": True,
            "bindings": {"%replacement": {"resolved": True, "entries": {}}},
        })

    def test_unloadable_helper_module_is_explicitly_unresolved(self):
        self.writer_helpers.unlink()
        helpers = self.dump()["native_write_helpers"]
        for key, fact in helpers.items():
            self.assertFalse(fact["resolved"])
            self.assertEqual(fact["reason"], "write_helper_load_failed")
            self.assertEqual(
                fact["requested_binding"],
                {"write_value": "Image::ExifTool::WriteValue",
                 "check_value": "Image::ExifTool::CheckValue",
                 "sanitize": "Image::ExifTool::Sanitize",
                 "conv_inv": "Image::ExifTool::ConvInv",
                 "set_new_value": "Image::ExifTool::SetNewValue",
                 "find_tag_info": "Image::ExifTool::TagLookup::FindTagInfo"}[key],
            )

    def test_write_only_source_mutation_changes_sidecar_not_read_projection(self):
        before = self.dump()
        self.write_exif("ExifIFD")
        after = self.dump()
        self.assertEqual(before["modules"], after["modules"])
        first = self.sidecar(before)["rows"]["316"]
        second = self.sidecar(after)["rows"]["316"]
        self.assertEqual(first["write_controls"]["WriteGroup"]["value"], "IFD0")
        self.assertEqual(second["write_controls"]["WriteGroup"]["value"], "ExifIFD")

    def test_writer_body_mutation_changes_authenticated_fact(self):
        before = self.dump()
        self.write_writer("return 2;")
        after = self.dump()
        first = self.sidecar(before)["effective_write_proc"]["effective"]
        second = self.sidecar(after)["effective_write_proc"]["effective"]
        self.assertTrue(first["resolved"])
        self.assertTrue(second["resolved"])
        self.assertNotEqual(first["__deparse"], second["__deparse"])
        self.assertNotEqual(first["source_sha256"], second["source_sha256"])
        self.assertEqual(before["modules"], after["modules"])

    def test_missing_autoload_implementation_is_explicitly_unresolved(self):
        self.writer.unlink()
        table = self.sidecar(self.dump())
        effective = table["effective_write_proc"]["effective"]
        self.assertFalse(effective["resolved"])
        self.assertEqual(effective["reason"], "autoload_load_failed")
        self.assertEqual(effective["autoload_file"], "Image/ExifTool/WriteExif.pl")


if __name__ == "__main__":
    unittest.main()
