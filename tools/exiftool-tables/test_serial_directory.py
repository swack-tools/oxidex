"""Checks for the staged source-only serial layout inventory."""

import copy
import json
import os
from pathlib import Path
import subprocess
import unittest

import serial_directory
import serial_directory_facts


PINNED_DUMP = os.environ.get("OXIDEX_TABLES_JSON")

# Self-contained source-shaped fixture: default tests cover the closed row
# grammar without requiring a local ExifTool installation. The native class
# below requires a caller-supplied recorded dump and validates its provenance.
PROCESSOR_BODY = '($$$) {\n    package Image::ExifTool::Canon;\n    use strict;\n    (my($et, $dirInfo, $tagTablePtr) = @_);\n    (my($dataPt) = $dirInfo->{\'DataPt\'});\n    (my($offset) = $dirInfo->{\'DirStart\'});\n    (my($size) = $dirInfo->{\'DirLen\'});\n    (my($base) = ($dirInfo->{\'Base\'} || 0));\n    (my($verbose) = $et->Options(\'Verbose\'));\n    (my($dataPos) = ($dirInfo->{\'DataPos\'} || 0));\n    (my($unknown) = $et->Options(\'Unknown\', 1));\n    ($et->{\'NO_UNKNOWN\'} = 1);\n    ($verbose and $et->VerboseDir(\'SerialData\', (undef), $size));\n    (my($defaultFormat) = ($tagTablePtr->{\'FORMAT\'} || \'int8u\'));\n    my($index, %val);\n    (my($pos) = 0);\n    for (($index = 0); ($tagTablePtr->{$index} and ($pos <= $size)); (++$index)) {\n        ((my $tagInfo = $et->GetTagInfo($tagTablePtr, $index)) or (last));\n        (my($format) = $tagInfo->{\'Format\'});\n        (my($count) = 1);\n        if ($format) {\n            if (($format =~ /(.*)\\[(.*)\\]/)) {\n                ($format = $1);\n                ($count = $2);\n                ($count = eval($count));\n                ($@ and (warn(("Format $tagInfo->{\'Name\'}: $@")), (last)));\n            } elsif (($format eq \'string\')) {\n                ($count = (($size > $pos) ? ($size - $pos) : 0));\n            }\n        } else {\n            ($format = $defaultFormat);\n        }\n        (my($len) = ((&Image::ExifTool::FormatSize($format) || 1) * $count));\n        ((($pos + $len) > $size) and (last));\n        (my($val) = ReadValue($dataPt, ($pos + $offset), $format, $count, ($size - $pos)));\n        (defined($val) or (last));\n        if ($verbose) {\n            $et->VerboseInfo($index, $tagInfo, \'Index\', $index, \'Table\', $tagTablePtr, \'Value\', $val, \'DataPt\', $dataPt, \'Size\', $len, \'Start\', ($pos + $offset), \'Addr\', ((($pos + $offset) + $base) + $dataPos), \'Format\', $format, \'Count\', $count);\n        }\n        ($val{$index} = $val);\n        if ($tagInfo->{\'SubDirectory\'}) {\n            (my($subTablePtr) = GetTagTable($tagInfo->{\'SubDirectory\'}{\'TagTable\'}));\n            (my(%dirInfo) = (\'DataPt\', (\\$val), \'DataPos\', ($dataPos + $pos), \'DirStart\', 0, \'DirLen\', length($val)));\n            $et->ProcessDirectory((\\%dirInfo), $subTablePtr);\n        } elsif ((not($tagInfo->{\'Unknown\'}) or $unknown)) {\n            ($count and (my($key) = $et->FoundTag($tagInfo, $val)));\n            if ($key) {\n                ($et->{\'OPTIONS\'}{\'SaveFormat\'} and ($et->{\'TAG_EXTRA\'}{$key}{\'G6\'} = $format));\n                ($et->{\'OPTIONS\'}{\'SaveBin\'} and ($et->{\'TAG_EXTRA\'}{$key}{\'BinVal\'} = substr($$dataPt, ($pos + $offset), $len)));\n            }\n        }\n        ($pos += $len);\n    }\n    $et->Options(\'Unknown\', $unknown);\n    delete $et->{\'NO_UNKNOWN\'};\n    (return 1);\n}'

# The reporting branch of PROCESSOR_BODY (grammar V1), and the same branch as
# captured from a processor without the SaveFormat/SaveBin stores (grammar V0).
V1_REPORTING_BLOCK = (
    "            ($count and (my($key) = $et->FoundTag($tagInfo, $val)));\n"
    "            if ($key) {\n"
    "                ($et->{'OPTIONS'}{'SaveFormat'} and ($et->{'TAG_EXTRA'}{$key}{'G6'} = $format));\n"
    "                ($et->{'OPTIONS'}{'SaveBin'} and ($et->{'TAG_EXTRA'}{$key}{'BinVal'} = "
    "substr($$dataPt, ($pos + $offset), $len)));\n"
    "            }\n"
)
V0_REPORTING_BLOCK = "            ($count and $et->FoundTag($tagInfo, $val));\n"
assert PROCESSOR_BODY.count(V1_REPORTING_BLOCK) == 1


def processor(body=PROCESSOR_BODY):
    return {
        "__perl": "CODE",
        "__name": "Image::ExifTool::Fixture::ProcessSerialData",
        "resolved": True,
        "__deparse": body,
        "source_file": "Image/ExifTool/Fixture.pm",
        "source_sha256": "a" * 64,
    }


def table(process=PROCESSOR_BODY):
    return {
        "meta": {
            "FORMAT": "int16u",
            "GROUPS": {"0": "MakerNotes", "2": "Camera"},
            "VARS": {"ID_LABEL": "Sequence"},
            "PROCESS_PROC": processor(process),
        },
        "tags": {
            "0": {"Name": "Count", "RawConv": {"kind": "expr", "expr": "$$self{Seen}=$val"}},
            "1": {"Name": "Signed", "Format": "int16s[$val{0}]"},
            "2": {
                "Name": "Words",
                "Format": "int16s[int(($val{0}+15)/16)]",
                "PrintConv": {"kind": "expr", "expr": "Image::ExifTool::DecodeBits($val, undef, 16)"},
            },
            "3": {
                "_variants": [
                    {"Name": "NonEos", "Condition": "$$self{Model} !~ /EOS/"},
                    {"Name": "Skipped", "Condition": "$$self{Model} !~ /EOS/", "Format": "int16u[8]", "Unknown": "1"},
                ]
            },
            "4": {"Name": "Primary"},
        },
    }


class SerialDescriptorTests(unittest.TestCase):
    def compile(self, document=None):
        return serial_directory.compile_serial_inventory("Fixture", "Serial", document or table())

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL"), "set EXIFTOOL_PERL for native flag truth")
    def test_reporting_flags_match_native_perl_scalar_truth(self):
        values = [None, False, True, 0, 1, -1, 0.0, 0.5, "", "0", "00", "0.0", "1", "false"]
        native = subprocess.run(
            [os.environ["EXIFTOOL_PERL"], "-MJSON::PP", "-e",
             "local $/; my $v=decode_json(<STDIN>); "
             "print encode_json([map { $_ ? JSON::PP::true() : JSON::PP::false() } @$v]);"],
            input=json.dumps(values), text=True, capture_output=True, check=True, timeout=15,
        )
        self.assertEqual(native.stderr, "")
        for value, expected in zip(values, json.loads(native.stdout), strict=True):
            for source, output in (("Unknown", "unknown"), ("Binary", "binary"), ("List", "list")):
                with self.subTest(source=source, value=value):
                    document = table()
                    document["tags"]["0"][source] = value
                    alternative = self.compile(document)["entries"][0]["alternatives"][0]
                    self.assertEqual(alternative["flags"][output], expected)
        for source in ("Unknown", "Binary", "List"):
            document = table()
            document["tags"]["0"][source] = {"__perl": "CODE"}
            alternative = self.compile(document)["entries"][0]["alternatives"][0]
            self.assertEqual(alternative["refusals"], ["serial_row_shape"])
            self.assertIn("not a literal scalar", alternative["native_refusal"])

    def test_descriptor_captures_order_raw_count_domain_and_refusal(self):
        descriptor = self.compile()
        self.assertEqual(descriptor["version"], 2)
        self.assertEqual(descriptor["runtime_status"], "inventory_only_no_reader_or_route")
        self.assertEqual(descriptor["processor"]["prior_value_domain"], "raw_read_value_before_conversion_or_reporting")
        self.assertEqual(
            descriptor["processor"]["effects_in_order"][-3:],
            ["serial cursor advance", "unknown option restore", "unknown generation cleanup"],
        )
        self.assertEqual(descriptor["entries"][1]["alternatives"][0]["format"]["count"],
                         {"kind": "prior_raw_value", "serial_index": 0})
        self.assertEqual(descriptor["entries"][0]["alternatives"][0]["raw_conv"],
                         {"kind": "set_member", "member": "Seen"})
        self.assertEqual(descriptor["entries"][2]["alternatives"][0]["format"]["count"],
                         {"kind": "floor_div_prior_raw_value", "serial_index": 0, "add": 15, "divisor": 16, "trailing_add": 0})
        self.assertEqual(descriptor["gate_a"]["blocked_by"], [])
        alternative = descriptor["entries"][3]["alternatives"][0]
        self.assertEqual(alternative["condition"]["missing_member"], "empty_string_for_string_ops")
        self.assertEqual(alternative["refusals"], [])
        self.assertEqual(descriptor["entries"][3]["alternatives"][1]["flags"]["unknown"], True)

    def test_mutated_supported_operands_produce_a_new_descriptor_and_stale_rejects(self):
        baseline = self.compile()
        changed = table()
        changed["tags"]["2"]["Format"] = "int16s[int(($val{0}+31)/32)]"
        fresh = self.compile(changed)
        self.assertEqual(fresh["entries"][2]["alternatives"][0]["format"]["count"],
                         {"kind": "floor_div_prior_raw_value", "serial_index": 0, "add": 31, "divisor": 32, "trailing_add": 0})
        self.assertEqual(
            serial_directory.stale_reason(baseline, "Fixture", "Serial", changed),
            "native serial descriptor or source identity differs",
        )

    def test_unsupported_count_expression_is_named_not_approximated(self):
        changed = table()
        changed["tags"]["1"]["Format"] = "int16s[$val{0} * 2]"
        descriptor = self.compile(changed)
        alternative = descriptor["entries"][1]["alternatives"][0]
        self.assertEqual(alternative["refusals"], ["serial_row_shape"])
        self.assertEqual(alternative["native_refusal"],
                         "serial Format count expression is outside the closed prior-value grammar")
        self.assertEqual(descriptor["gate_a"]["blocked_by"], [["serial_row_shape", 1]])

    def test_one_bad_variant_does_not_hide_its_supported_sibling(self):
        changed = table()
        changed["tags"]["3"]["_variants"][1]["Format"] = "int16s[$val{0} * 2]"
        descriptor = self.compile(changed)
        alternatives = descriptor["entries"][3]["alternatives"]
        self.assertEqual(alternatives[0]["name"], "NonEos")
        self.assertEqual(alternatives[0]["refusals"], [])
        self.assertEqual(alternatives[1]["name"], "Skipped")
        self.assertEqual(alternatives[1]["refusals"], ["serial_row_shape"])
        self.assertIn(["serial_row_shape", 1], descriptor["gate_a"]["blocked_by"])

    def test_rust_emitter_uses_descriptor_facts_and_sidecars_every_withheld_row(self):
        population = serial_directory.compile_serial_population({"modules": {"Fixture": {"tables": {"Serial": table()}}}})
        source = serial_directory.serial_rust_source(population)
        self.assertIn("pub static SERIAL_FIXTURE_SERIAL: SerialTable", source)
        self.assertIn('group0: "MakerNotes", group1: "Fixture", group2: "Camera",', source)
        self.assertIn('format: Fmt::Int16s', source)
        self.assertIn('raw_conv: Some(RawConvEffect::SetMember { member: "Seen" })', source)
        self.assertIn('SerialCount::FloorDivPriorRaw { serial_index: 0, add: 15, divisor: 16, trailing_add: 0 }', source)
        self.assertIn('print_conv: SerialPrintConv::DecodeBitsWords { bits_per_word: 16 }', source)
        self.assertIn('missing_member: SerialMissingMember::EmptyStringForStringOps', source)
        self.assertIn('gate_a: GateA { blocked_by: &[] }', source)
        self.assertNotIn('module: "Fixture", table: "Serial", serial_index:', source)

    def test_rust_emitter_uses_native_false_group_defaults(self):
        changed = table()
        changed["meta"]["GROUPS"] = {"0": "0", "1": "", "2": "0"}
        population = serial_directory.compile_serial_population({"modules": {"Fixture": {"tables": {"Serial": changed}}}})
        source = serial_directory.serial_rust_source(population)
        self.assertIn('group0: "Fixture", group1: "Fixture", group2: "Other",', source)

    def test_rust_emitter_uses_first_nested_module_component_for_native_defaults(self):
        changed = table()
        population = serial_directory.compile_serial_population(
            {"modules": {"Fixture::Nested": {"tables": {"Serial": changed}}}}
        )
        source = serial_directory.serial_rust_source(population)
        self.assertIn('module: "Fixture::Nested", table: "Serial", group0: "MakerNotes", group1: "Fixture", group2: "Camera",', source)

    def test_rust_emitter_keeps_refused_table_in_table_sidecar(self):
        malformed = table()
        malformed["tags"] = []
        population = serial_directory.compile_serial_population({"modules": {"Fixture": {"tables": {"Malformed": malformed}}}})
        source = serial_directory.serial_rust_source(population)
        self.assertNotIn("SERIAL_FIXTURE_MALFORMED", source)
        self.assertIn('OmittedSerialNativeTable { module: "Fixture", table: "Malformed", reasons: &["serial_descriptor_refused"] }', source)

    def test_rust_emitter_refuses_a_format_not_supported_by_the_shared_reader(self):
        changed = table()
        changed["tags"]["0"]["Format"] = "int8s"
        population = serial_directory.compile_serial_population({"modules": {"Fixture": {"tables": {"Serial": changed}}}})
        source = serial_directory.serial_rust_source(population)
        self.assertIn('serial_index: 0, variant: false, alternative: 0, name: Some("Count"), reasons: &["serial_emitter_format"]', source)
        self.assertIn('("serial_emitter_format", 1)', source)

    def test_source_order_and_identity_refuse_malformed_processor(self):
        bad_order = PROCESSOR_BODY.replace("($val{$index} = $val);", "($et->FoundTag($tagInfo, $val));\n    ($val{$index} = $val);")
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "complete executable grammar"):
            self.compile(table(bad_order))
        unresolved = table()
        unresolved["meta"]["PROCESS_PROC"]["resolved"] = False
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "unresolved"):
            self.compile(unresolved)
        escaped = table()
        escaped["meta"]["PROCESS_PROC"]["source_file"] = "../Canon.pm"
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "library-relative"):
            self.compile(escaped)
        extra_effect = table(PROCESSOR_BODY.replace("($pos += $len);", "$et->Warn('side effect');\n    ($pos += $len);"))
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "complete executable grammar"):
            self.compile(extra_effect)
        quoted_fake = table(PROCESSOR_BODY.replace(
            "(my($unknown) = $et->Options('Unknown', 1));",
            "(my($unknown) = 1); my($note) = \"Options('Unknown',1)\";",
        ))
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "complete executable grammar"):
            self.compile(quoted_fake)

    def test_processor_without_option_gated_tag_extra_compiles_the_same_descriptor(self):
        # The shared processor as captured without SaveFormat/SaveBin stores
        # (grammar V0). Only the two source fingerprints that cover the
        # processor body may differ: every row, gate and modeled effect must be
        # identical to the V1 descriptor.
        v0_body = PROCESSOR_BODY.replace(V1_REPORTING_BLOCK, V0_REPORTING_BLOCK)
        self.assertNotEqual(v0_body, PROCESSOR_BODY)
        v1 = self.compile(table(PROCESSOR_BODY))
        v0 = self.compile(table(v0_body))
        self.assertEqual(v0["processor"]["source_body_sha256"], serial_directory_facts.deparse_sha256(v0_body))
        for fingerprint in (("processor", "source_body_sha256"), ("table_facts", "native_table_sha256")):
            section, key = fingerprint
            self.assertNotEqual(v0[section].pop(key), v1[section].pop(key), fingerprint)
        self.assertEqual(v0, v1)

    def test_partial_option_gated_tag_extra_excisions_refuse(self):
        # Acceptance is by whole-body token stream, not by "V1 minus something":
        # dropping only one of the stores, or keeping the key capture with no
        # consumer, is neither grammar and must refuse.
        save_format = (
            "\n                ($et->{'OPTIONS'}{'SaveFormat'} and ($et->{'TAG_EXTRA'}{$key}{'G6'} = $format));"
        )
        save_bin = (
            "\n                ($et->{'OPTIONS'}{'SaveBin'} and ($et->{'TAG_EXTRA'}{$key}{'BinVal'} = "
            "substr($$dataPt, ($pos + $offset), $len)));"
        )
        self.assertIn(save_format, PROCESSOR_BODY)
        self.assertIn(save_bin, PROCESSOR_BODY)
        partials = {
            "only SaveFormat removed": PROCESSOR_BODY.replace(save_format, ""),
            "only SaveBin removed": PROCESSOR_BODY.replace(save_bin, ""),
            "key captured, block removed": PROCESSOR_BODY.replace(
                V1_REPORTING_BLOCK, "            ($count and (my($key) = $et->FoundTag($tagInfo, $val)));\n"),
            "V0 call without the count guard": PROCESSOR_BODY.replace(
                V1_REPORTING_BLOCK, "            $et->FoundTag($tagInfo, $val);\n"),
        }
        for label, body in partials.items():
            with self.subTest(label):
                self.assertNotEqual(body, PROCESSOR_BODY)
                with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "complete executable grammar"):
                    self.compile(table(body))

    def test_unmodeled_table_and_row_properties_are_preserved_and_gate_refused(self):
        changed = table()
        changed["meta"]["PRIORITY"] = "0"
        descriptor = self.compile(changed)
        self.assertEqual(descriptor["table_facts"]["unmodeled"], {"PRIORITY": "0"})
        self.assertIn(["serial_table_priority", 1], descriptor["gate_a"]["blocked_by"])
        for property_name in ("Mask", "ByteOrder", "Hook", "DataMember"):
            row_changed = table()
            row_changed["tags"]["0"][property_name] = "native"
            row = self.compile(row_changed)["entries"][0]["alternatives"][0]
            self.assertEqual(row["native"][property_name], "native")
            self.assertIn(f"serial_row_property_{property_name}", row["refusals"])

    def test_raw_fact_hashing_is_independent_of_recognition(self):
        descriptor = self.compile()
        raw = table()
        self.assertEqual(
            descriptor["table_facts"]["native_table_sha256"],
            serial_directory_facts.canonical_json_sha256({"meta": raw["meta"], "tags": raw["tags"]}),
        )
        with self.assertRaises(serial_directory_facts.SerialFactRefused):
            serial_directory_facts.deparse_sha256(None)

    def test_population_keeps_empty_refused_and_nonserial_tables_visible_or_out(self):
        empty = table()
        empty["tags"] = {}
        malformed = table()
        malformed["tags"] = []
        nonserial = table()
        nonserial["meta"]["PROCESS_PROC"]["__name"] = "Image::ExifTool::Fixture::OtherProcessor"
        document = {"modules": {"Fixture": {"tables": {
            "Serial": table(), "Empty": empty, "Malformed": malformed, "Other": nonserial,
        }}}}
        population = serial_directory.compile_serial_population(document)
        self.assertEqual(population["summary"]["selected_tables"], 3)
        self.assertEqual(population["summary"]["descriptor_compiled_tables"], 2)
        self.assertEqual(population["summary"]["descriptor_refused_tables"], 1)
        self.assertEqual(population["summary"]["empty_selected_tables"], 1)
        outcomes = {(item["table"], item["outcome"]) for item in population["tables"]}
        self.assertIn(("Empty", "descriptor_compiled"), outcomes)
        self.assertIn(("Malformed", "descriptor_refused"), outcomes)
        self.assertNotIn(("Other", "descriptor_compiled"), outcomes)

    def test_population_rejects_missing_module_or_table_dictionaries(self):
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "module dictionary"):
            serial_directory.compile_serial_population({})
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "table dictionary"):
            serial_directory.compile_serial_population({"modules": {"Fixture": {}}})

    def test_first_serial_index_gap_is_not_treated_as_reachable(self):
        changed = table()
        changed["tags"] = {"1": changed["tags"]["1"]}
        descriptor = self.compile(changed)
        self.assertEqual(descriptor["first_unreachable_serial_index"], 0)
        self.assertIn(["serial_gap", 1], descriptor["gate_a"]["blocked_by"])


@unittest.skipUnless(PINNED_DUMP, "set OXIDEX_TABLES_JSON to a pinned native dump")
class PinnedSerialInventory(unittest.TestCase):
    def setUp(self):
        path = Path(PINNED_DUMP)
        self.assertTrue(path.is_file(), f"native dump is not a file: {path}")
        self.document = json.loads(path.read_text())
        self.table = self.document["modules"]["Canon"]["tables"]["AFInfo"]
        process = self.table["meta"]["PROCESS_PROC"]
        self.assertEqual(process.get("__name"), "Image::ExifTool::Canon::ProcessSerialData")
        self.assertTrue(process.get("resolved"), "dump must contain resolved processor provenance")

    def compile(self, source=None):
        return serial_directory.compile_serial_inventory("Canon", "AFInfo", source or self.table)

    def test_pinned_afinfo_captures_full_order_and_supported_signed_operands(self):
        descriptor = self.compile()
        self.assertEqual([entry["serial_index"] for entry in descriptor["entries"]], list(range(13)))
        self.assertEqual(descriptor["gate_a"]["blocked_by"], [])
        self.assertEqual(
            descriptor["entries"][8]["alternatives"][0]["format"]["count"],
            {"kind": "prior_raw_value", "serial_index": 0},
        )
        self.assertEqual(
            descriptor["entries"][10]["alternatives"][0]["format"]["count"],
            {"kind": "floor_div_prior_raw_value", "serial_index": 0, "add": 15, "divisor": 16, "trailing_add": 0},
        )
        self.assertEqual(descriptor["entries"][11]["alternatives"][0]["condition"]["missing_member"], "empty_string_for_string_ops")
        self.assertEqual(descriptor["entries"][11]["alternatives"][0]["refusals"], [])

    def test_pinned_afinfo2_emits_enum_member_effect_and_trailing_count(self):
        """Exercise the three newly admitted operands from captured native facts.

        This is deliberately AFInfo2 rather than a fixture: the enum's absent
        key is native `GetValue` behavior, the member name is the actual
        RawConv destination, and the final `+1` occurs after the count
        division in the source Format expression.
        """
        table = self.document["modules"]["Canon"]["tables"]["AFInfo2"]
        descriptor = serial_directory.compile_serial_inventory("Canon", "AFInfo2", table)
        mode = descriptor["entries"][1]["alternatives"][0]
        self.assertEqual(mode["conversion"]["kind"], "shared_int_enum")
        mapping = dict(mode["conversion"]["map"])
        self.assertEqual(mapping[2], "Single-point AF")
        self.assertNotIn(3, mapping, "native uses its unknown-value fallback for unmapped mode 3")

        count = descriptor["entries"][2]["alternatives"][0]
        self.assertEqual(
            count["format"]["format"], "int16u",
            "the dynamic-count controller is native unsigned; signed payload arrays are distinct",
        )
        self.assertEqual(count["raw_conv"], {"kind": "set_member", "member": "NumAFPoints"})

        trailing = descriptor["entries"][13]["alternatives"][1]
        self.assertEqual(
            trailing["format"]["count"],
            {"kind": "floor_div_prior_raw_value", "serial_index": 2,
             "add": 15, "divisor": 16, "trailing_add": 1},
        )

        source = serial_directory.serial_rust_source(
            serial_directory.compile_serial_population(self.document)
        )
        self.assertIn(
            'name: "AFAreaMode", format: SerialFormat { format: Fmt::Int16u, '
            'count: SerialCount::Fixed { value: 1 } },',
            source,
        )
        self.assertIn('print_conv: SerialPrintConv::Shared(PrintConv::IntEnum(&[', source)
        self.assertIn(
            'name: "NumAFPoints", format: SerialFormat { format: Fmt::Int16u, '
            'count: SerialCount::Fixed { value: 1 } },',
            source,
        )
        self.assertIn(
            'raw_conv: Some(RawConvEffect::SetMember { member: "NumAFPoints" })', source,
        )
        self.assertIn(
            'SerialCount::FloorDivPriorRaw { serial_index: 2, add: 15, divisor: 16, '
            'trailing_add: 1 }',
            source,
        )

    def test_fresh_descriptor_is_canonical_json(self):
        # Recorded reports are regenerated by the sanctioned whole-dump path.
        # This source test must not require (or hand-edit) a generated report
        # while a descriptor-schema change is being authored.
        self.assertEqual(
            json.loads(serial_directory.descriptor_json(self.compile())),
            self.compile(),
        )

    def test_copied_source_facts_mutations_stale_or_refuse(self):
        baseline = self.compile()
        changed = copy.deepcopy(self.table)
        changed["tags"]["8"]["Format"] = "int16s[$val{1}]"
        self.assertEqual(
            serial_directory.stale_reason(baseline, "Canon", "AFInfo", changed),
            "native serial descriptor or source identity differs",
        )
        unsupported = copy.deepcopy(self.table)
        unsupported["tags"]["10"]["Format"] = "int16s[int(($val{0}+15)/0)]"
        descriptor = self.compile(unsupported)
        self.assertIn(["serial_row_shape", 1], descriptor["gate_a"]["blocked_by"])

    def test_population_accounts_for_every_native_serial_selector_and_generalizes_to_real_audio(self):
        population = serial_directory.compile_serial_population(self.document)
        self.assertEqual(population["summary"], {
            "selected_tables": 8,
            "descriptor_compiled_tables": 8,
            "descriptor_refused_tables": 0,
            "empty_selected_tables": 0,
            "native_entries": 130,
            "native_alternatives": 132,
            "compiled_entries": 130,
            "compiled_alternatives": 132,
            "row_gate_clear_alternatives": 122,
            "row_gate_refused_alternatives": 10,
            "row_gate_clear_tables": 6,
            "row_gate_blocked_by": [
                ["serial_print_conv", 1],
                ["serial_print_conv_expr", 8],
                ["serial_row_shape", 1],
                ["serial_value_conv", 4],
            ],
            "table_gate_blocked_by": [["serial_table_priority", 1]],
            "table_gate_blocked_tables": 1,
        })
        rows = {(row["module"], row["table"]): row for row in population["tables"]}
        self.assertEqual(rows[("Real", "AudioV3")]["descriptor"]["gate_a"]["blocked_by"], [])
        self.assertEqual(
            json.loads(serial_directory.population_json(population)),
            population,
        )

    def test_real_audio_pair_emits_dynamic_raw_counts_and_no_omissions(self):
        source = serial_directory.serial_rust_source(serial_directory.compile_serial_population(self.document))
        self.assertIn("pub static SERIAL_REAL_AUDIOV3: SerialTable", source)
        self.assertIn("pub static SERIAL_REAL_AUDIOV4: SerialTable", source)
        self.assertIn('name: "Title", format: SerialFormat { format: Fmt::Str(1), count: SerialCount::PriorRaw { serial_index: 4 } }', source)
        self.assertIn('name: "Title", format: SerialFormat { format: Fmt::Str(1), count: SerialCount::PriorRaw { serial_index: 23 } }', source)
        self.assertIn('name: "Artist", format: SerialFormat { format: Fmt::Str(1), count: SerialCount::PriorRaw { serial_index: 6 } }', source)
        self.assertIn('name: "Artist", format: SerialFormat { format: Fmt::Str(1), count: SerialCount::PriorRaw { serial_index: 25 } }', source)
        self.assertNotIn('module: "Real", table: "AudioV3", serial_index:', source)
        self.assertNotIn('module: "Real", table: "AudioV4", serial_index:', source)


PINNED_SOURCE = os.environ.get("OXIDEX_PINNED_EXIFTOOL")
CANONICAL_PERL = os.environ.get("EXIFTOOL_PERL")
DUMP_TABLES = Path(__file__).resolve().with_name("dump_tables.pl")


@unittest.skipUnless(PINNED_SOURCE and CANONICAL_PERL,
                     "set OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_PERL for copied-native serial checks")
class CopiedNativeSerialSource(unittest.TestCase):
    """The compiler consumes a dump of copied Perl, never a hand-built Rust fact."""

    def dump(self, lib, module="Canon", table="AFInfo"):
        import subprocess
        result = subprocess.run(
            [CANONICAL_PERL, str(DUMP_TABLES), str(lib), module],
            check=True, text=True, capture_output=True,
        )
        return json.loads(result.stdout)["modules"][module]["tables"][table]

    def copied(self, before, after, relative_source="Image/ExifTool/Canon.pm"):
        import shutil
        from tempfile import TemporaryDirectory
        tmp = TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name) / "lib"
        shutil.copytree(Path(PINNED_SOURCE) / "lib", root)
        source = root / relative_source
        text = source.read_text()
        self.assertIn(before, text)
        source.write_text(text.replace(before, after, 1))
        return root

    def test_native_get_tag_table_defaults_omitted_group_one_to_owner_module(self):
        native = subprocess.run(
            [CANONICAL_PERL, "-I", str(Path(PINNED_SOURCE) / "lib"), "-MJSON::PP", "-MImage::ExifTool", "-e",
             'my $table = Image::ExifTool::GetTagTable("Image::ExifTool::Canon::AFInfo") '
             'or die "missing native Canon::AFInfo table\\n"; print encode_json($table->{GROUPS}), "\\n";'],
            text=True, capture_output=True, check=True, timeout=15,
        )
        self.assertEqual(native.stderr, "")
        self.assertEqual(json.loads(native.stdout), {"0": "MakerNotes", "1": "Canon", "2": "Camera"})

    def test_native_get_tag_table_uses_first_component_of_nested_owner_module(self):
        from tempfile import TemporaryDirectory
        with TemporaryDirectory() as directory:
            lib = Path(directory) / "lib"
            package = lib / "Image" / "ExifTool" / "Fixture" / "Nested.pm"
            package.parent.mkdir(parents=True)
            package.write_text(
                "package Image::ExifTool::Fixture::Nested;\n"
                "use strict;\n"
                "our %Main = ( GROUPS => { 0 => 'MakerNotes', 2 => 'Camera' } );\n"
                "1;\n",
                encoding="utf-8",
            )
            native = subprocess.run(
                [CANONICAL_PERL, "-I", str(lib), "-I", str(Path(PINNED_SOURCE) / "lib"),
                 "-MJSON::PP", "-MImage::ExifTool", "-e",
                 'my $table = Image::ExifTool::GetTagTable("Image::ExifTool::Fixture::Nested::Main") '
                 'or die "missing native nested table\\n"; print encode_json($table->{GROUPS}), "\\n";'],
                text=True, capture_output=True, check=True, timeout=15,
            )
        self.assertEqual(native.stderr, "")
        self.assertEqual(json.loads(native.stdout), {"0": "MakerNotes", "1": "Fixture", "2": "Camera"})

    def test_source_count_mutation_changes_fresh_descriptor_and_rejects_stale(self):
        baseline_source = self.dump(Path(PINNED_SOURCE) / "lib")
        baseline = serial_directory.compile_serial_inventory("Canon", "AFInfo", baseline_source)
        changed_source = self.dump(self.copied("int16s[$val{0}]", "int16s[$val{1}]"))
        changed = serial_directory.compile_serial_inventory("Canon", "AFInfo", changed_source)
        self.assertEqual(changed["entries"][8]["alternatives"][0]["format"]["count"],
                         {"kind": "prior_raw_value", "serial_index": 1})
        self.assertEqual(serial_directory.stale_reason(baseline, "Canon", "AFInfo", changed_source),
                         "native serial descriptor or source identity differs")

    def test_source_decodebits_mutation_remains_named_refusal(self):
        changed_source = self.dump(self.copied(
            "Image::ExifTool::DecodeBits($val, undef, 16)",
            "Image::ExifTool::DecodeBits($val, undef, 32)",
        ))
        descriptor = serial_directory.compile_serial_inventory("Canon", "AFInfo", changed_source)
        self.assertEqual(descriptor["gate_a"]["blocked_by"], [["serial_print_conv_expr", 1]])

    def test_real_audio_copied_source_count_mutation_changes_emitted_literal(self):
        baseline_table = self.dump(Path(PINNED_SOURCE) / "lib", "Real", "AudioV3")
        baseline = serial_directory.compile_serial_population({"modules": {"Real": {"tables": {"AudioV3": baseline_table}}}})
        changed_table = self.dump(
            self.copied(
                "5  => { Name => 'Title',          Format => 'string[$val{4}]' },",
                "5  => { Name => 'Title',          Format => 'string[$val{6}]' },",
                "Image/ExifTool/Real.pm",
            ),
            "Real", "AudioV3",
        )
        changed = serial_directory.compile_serial_population({"modules": {"Real": {"tables": {"AudioV3": changed_table}}}})
        baseline_source = serial_directory.serial_rust_source(baseline)
        changed_source = serial_directory.serial_rust_source(changed)
        self.assertIn("SerialCount::PriorRaw { serial_index: 4 }", baseline_source)
        self.assertIn("SerialCount::PriorRaw { serial_index: 6 }", changed_source)
        self.assertNotEqual(baseline_source, changed_source)

    # The pinned source's reporting statement, and the same statement written
    # without the SaveFormat/SaveBin TAG_EXTRA stores (grammar V0).
    V1_SOURCE_REPORTING = (
        "            my $key = $et->FoundTag($tagInfo, $val) if $count;\n"
        "            if ($key) {\n"
        "                $$et{TAG_EXTRA}{$key}{G6} = $format if $$et{OPTIONS}{SaveFormat};\n"
        "                $$et{TAG_EXTRA}{$key}{BinVal} = substr($$dataPt, $pos+$offset, $len) if $$et{OPTIONS}{SaveBin};\n"
        "            }\n"
    )
    V0_SOURCE_REPORTING = "            $et->FoundTag($tagInfo, $val) if $count;\n"

    def test_source_without_option_gated_tag_extra_compiles_identical_rows(self):
        baseline_table = self.dump(Path(PINNED_SOURCE) / "lib")
        baseline = serial_directory.compile_serial_inventory("Canon", "AFInfo", baseline_table)
        changed_table = self.dump(self.copied(self.V1_SOURCE_REPORTING, self.V0_SOURCE_REPORTING))
        changed = serial_directory.compile_serial_inventory("Canon", "AFInfo", changed_table)
        self.assertNotEqual(changed["processor"]["source_body_sha256"], baseline["processor"]["source_body_sha256"])
        self.assertEqual(changed["entries"], baseline["entries"])
        self.assertEqual(changed["gate_a"], baseline["gate_a"])
        self.assertEqual(changed["processor"]["effects_in_order"], baseline["processor"]["effects_in_order"])

    def test_source_with_one_tag_extra_store_removed_refuses(self):
        for store in (
            "                $$et{TAG_EXTRA}{$key}{G6} = $format if $$et{OPTIONS}{SaveFormat};\n",
            "                $$et{TAG_EXTRA}{$key}{BinVal} = substr($$dataPt, $pos+$offset, $len) if $$et{OPTIONS}{SaveBin};\n",
        ):
            with self.subTest(store=store.strip()):
                self.assertIn(store, self.V1_SOURCE_REPORTING)
                changed_table = self.dump(self.copied(
                    self.V1_SOURCE_REPORTING, self.V1_SOURCE_REPORTING.replace(store, "")))
                with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "complete executable grammar"):
                    serial_directory.compile_serial_inventory("Canon", "AFInfo", changed_table)

    def test_source_unknown_option_mutation_cannot_be_faked_by_a_quoted_string(self):
        changed_source = self.dump(self.copied(
            "my $unknown = $et->Options(Unknown => 1);",
            "my $unknown = 1; my $note = \"Options('Unknown',1)\";",
        ))
        with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "complete executable grammar"):
            serial_directory.compile_serial_inventory("Canon", "AFInfo", changed_source)

    def test_source_boundary_and_reporting_operator_mutations_refuse_fresh_generation(self):
        mutations = (
            ("for ($index=0; $$tagTablePtr{$index} and $pos <= $size; ++$index) {",
             "for ($index=0; $$tagTablePtr{$index} and $pos < $size; ++$index) {"),
            ("my $size = $$dirInfo{DirLen};", "my $size = $$dirInfo{DirStart};"),
            ("ReadValue($dataPt, $pos+$offset, $format, $count, $size-$pos);",
             "ReadValue($dataPt, $pos+$offset, $format, $count, $size+$pos);"),
            ("$verbose and $et->VerboseDir('SerialData', undef, $size);",
             "$verbose or $et->VerboseDir('SerialData', undef, $size);"),
            ("} elsif (not $$tagInfo{Unknown} or $unknown) {",
             "} elsif (not $$tagInfo{Unknown} and $unknown) {"),
        )
        for before, after in mutations:
            with self.subTest(before=before):
                changed_source = self.dump(self.copied(before, after))
                with self.assertRaisesRegex(serial_directory.SerialDirectoryRefused, "complete executable grammar"):
                    serial_directory.compile_serial_inventory("Canon", "AFInfo", changed_source)


if __name__ == "__main__":
    unittest.main()
