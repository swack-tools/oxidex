"""Source-only checks for the keyed-directory schema compiler.

No Rust reader is linked here.  These tests establish that source mutations
change emitted facts and that unavailable ProcessCanonRaw condition context is
withheld before a future caller can accidentally invent a retry.
"""

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

import conds
import keyed_directory
import verify


PROC = {"__perl": "CODE", "__name": "Image::ExifTool::CanonRaw::ProcessCanonRaw"}
BIN = {"__perl": "CODE", "__name": "Image::ExifTool::ProcessBinaryData"}
SERIAL = {"__perl": "CODE", "__name": "Image::ExifTool::Canon::ProcessSerialData"}


def doc(main):
    return {"modules": {"Any": {"tables": {
        "Main": {"meta": {"PROCESS_PROC": PROC, "GROUPS": {"0": "MakerNotes"}}, "tags": main},
        "MakeModel": {"meta": {"PROCESS_PROC": BIN}, "tags": {}},
        "ImageFormat": {"meta": {"PROCESS_PROC": BIN}, "tags": {}},
        "Serial": {"meta": {"PROCESS_PROC": SERIAL}, "tags": {}},
    }}}}


class Selection(unittest.TestCase):
    def test_selection_is_processor_identity_not_module_or_table(self):
        self.assertTrue(keyed_directory.is_keyed_directory_table({"PROCESS_PROC": PROC}))
        self.assertFalse(keyed_directory.is_keyed_directory_table({"PROCESS_PROC": BIN}))
        self.assertFalse(keyed_directory.is_keyed_directory_table({}))

    def test_dump_identity_shape_is_accepted(self):
        # This is the shape dump_tables.pl preserves for CanonRaw::Main.
        self.assertEqual(keyed_directory.processor_name({"PROCESS_PROC": PROC}), PROC["__name"])


class InitialContext(unittest.TestCase):
    def test_no_value_context_includes_valpt_format_count_and_assignment_tail(self):
        for condition in (
            "$valPt =~ /^x/",
            '$format eq "int16u"',
            "$count == 6",
            "($$self{Seen} = 1) and $count == 6",
        ):
            self.assertTrue(conds.needs_initial_get_tag_info_context(condition), condition)
        self.assertFalse(conds.needs_initial_get_tag_info_context("$$self{Model} =~ /EOS/"))

    def test_unavailable_condition_is_a_single_named_omission(self):
        src, stats = keyed_directory.generate(doc({
            "0x180b": {"Name": "SerialNumber", "Condition": "$count == 6"},
        }))
        self.assertIn('raw_id: "0x180b", variant: false, name: Some("SerialNumber"), native:', src)
        self.assertIn('reasons: &["condition"]', src)
        self.assertEqual(stats["keyed_condition"], 1)
        self.assertNotIn('name: "SerialNumber"', src.split("OMITTED_KEYED_NATIVE_ROWS", 1)[0])


class SourceDrivenFacts(unittest.TestCase):
    def test_count_preserves_undefined_zero_and_explicit_scalar_source_count(self):
        src, stats = keyed_directory.generate(doc({
            "0x1001": {"Name": "Implicit"},
            "0x1002": {"Name": "Zero", "Format": "int16u", "Count": 0},
            "0x1003": {"Name": "Three", "Format": "int16u", "Count": 3},
            "0x1004": {"Name": "Bad", "Format": "int16u", "Count": "many"},
        }))
        self.assertIn('raw_id: 0x1001, name: "Implicit", format: None, count: None', src)
        self.assertIn('raw_id: 0x1002, name: "Zero", format: Some(Fmt::Int16u), count: Some(0)', src)
        self.assertIn('raw_id: 0x1003, name: "Three", format: Some(Fmt::Int16u), count: Some(3)', src)
        self.assertIn('raw_id: "0x1004", variant: false, name: Some("Bad"), native:', src)
        self.assertIn('reasons: &["count"]', src)
        self.assertEqual(stats["keyed_count"], 1)

    def test_parent_id_and_target_mutations_change_schema_without_a_tag_rule(self):
        base = {"0x080a": {"Name": "CanonRawMakeModel", "SubDirectory": {"TagTable": "Image::ExifTool::Any::MakeModel"}}}
        moved = {"0x0809": base["0x080a"]}
        target = {"0x080a": {"Name": "CanonRawMakeModel", "SubDirectory": {"TagTable": "Image::ExifTool::Any::ImageFormat"}}}
        base_src, _ = keyed_directory.generate(doc(base))
        moved_src, _ = keyed_directory.generate(doc(moved))
        target_src, _ = keyed_directory.generate(doc(target))
        self.assertIn("raw_id: 0x080a", base_src)
        self.assertIn("raw_id: 0x0809", moved_src)
        self.assertIn('table: "ImageFormat"', target_src)
        self.assertNotIn('table: "MakeModel"', target_src)

    def test_same_table_directory_and_unwalked_edge_facts_are_explicit(self):
        src, stats = keyed_directory.generate(doc({
            "0x2804": {"Name": "ImageDescription", "SubDirectory": {}},
            "0x1803": {"Name": "ImageFormat", "SubDirectory": {
                "TagTable": "Image::ExifTool::Any::Serial", "Start": "$val", "Validate": "1",
                "ProcessProc": PROC,
            }},
        }))
        self.assertIn("KeyedEdge::SameTableDirectory", src)
        self.assertIn('unwalked: &["start", "validate", "process_proc", "target_processor"]', src)
        self.assertEqual(stats["keyed_edge_unwalked"], 1)

    def test_atomic_variants_keep_source_order(self):
        src, _ = keyed_directory.generate(doc({
            "0x180b": {"_variants": [
                {"Name": "D30", "Condition": "$$self{Model} =~ /D30/"},
                {"Name": "Other", "Condition": None},
            ]},
        }))
        self.assertLess(src.index('name: "D30"'), src.index('name: "Other"'))
        self.assertIn("KeyedVariantGroup", src)


class IndependentInventory(unittest.TestCase):
    def _artifact(self, tags):
        src, _ = keyed_directory.generate(doc(tags))
        temp = TemporaryDirectory()
        path = Path(temp.name) / "keyed_tables.rs"
        path.write_text(src, encoding="utf-8")
        self.addCleanup(temp.cleanup)
        return path

    def test_parser_accounts_for_numeric_ids_variants_enums_and_omissions(self):
        path = self._artifact({
            "0x1001": {"Name": "Mode", "PrintConv": {
                "kind": "enum", "map": {"0": "Off", "1": "On"}, "directives": {},
            }},
            "0x180b": {"Name": "Unavailable", "Condition": "$count == 6"},
            "0x180c": {"_variants": [
                {"Name": "D30", "Condition": "$$self{Model} =~ /D30/"},
                {"Name": "Other", "Condition": None},
            ]},
        })
        generated = verify.parse_keyed_rust(path)
        omissions = verify.parse_omitted_keyed_native_rows(path)
        self.assertEqual(generated.fields[("Any", "Main", "4097")], "Mode")
        self.assertEqual(generated.enums[("Any", "Main", "4097")], {"0": "Off", "1": "On"})
        self.assertEqual(generated.fields[("Any", "Main", "6156#0")], "D30")
        self.assertEqual(generated.fields[("Any", "Main", "6156#1")], "Other")
        self.assertTrue(omissions.present)
        self.assertEqual(omissions.rows[("Any", "Main", "6155")].reasons, ("condition",))

    def test_inventory_rejects_an_unauthenticated_or_stale_omission_once(self):
        path = self._artifact({"0x180b": {"Name": "Unavailable", "Condition": "$count == 6"}})
        generated = verify.parse_keyed_rust(path)
        omissions = verify.parse_omitted_keyed_native_rows(path)
        names = {("Any", "Main", "6155"): "Unavailable"}
        clean = verify.keyed_native_inventory(
            generated, omissions, names, {}, {}, {}, set(), set(),
            {("Any", "Main", "6155"): {"condition"}}, {},
            {("Any", "Main"): ("MakerNotes", "", "")}, {}, {},
            {("Any", "Main", "6155"): "$count == 6"},
        )
        self.assertEqual(clean.accounted, 1)
        bad = verify.keyed_native_inventory(
            generated, omissions, names, {}, {}, {}, set(), set(), {},
            {}, {("Any", "Main"): ("MakerNotes", "", "")}, {}, {}, {},
        )
        self.assertEqual(bad.missing, ())
        self.assertEqual(len(bad.bad_reasons), 1)

    def test_saved_parent_target_mutation_rejects_stale_executable_edge(self):
        # Mirrors native-keyed-fixtures/mutation-diffs/parent-target.diff:
        # Main 0x080a switches MakeModel to ImageFormat.  The tag name stays
        # the same, so a names-only inventory would have passed stale Rust.
        path = self._artifact({"0x080a": {
            "Name": "CanonRawMakeModel",
            "SubDirectory": {"TagTable": "Image::ExifTool::Any::MakeModel"},
        }})
        generated = verify.parse_keyed_rust(path)
        omissions = verify.parse_omitted_keyed_native_rows(path)
        key = ("Any", "Main", "2058")
        base = verify.keyed_native_inventory(
            generated, omissions, {key: "CanonRawMakeModel"}, {}, {}, {}, set(), set(), {},
            {}, {("Any", "Main"): ("MakerNotes", "", "")}, {},
            {key: {"tagtable": "Image::ExifTool::Any::MakeModel", "start": "", "validate": False, "processproc": False}}, {},
        )
        self.assertEqual(base.keyed_fact_mismatches, ())
        moved = verify.keyed_native_inventory(
            generated, omissions, {key: "CanonRawMakeModel"}, {}, {}, {}, set(), set(), {},
            {}, {("Any", "Main"): ("MakerNotes", "", "")}, {},
            {key: {"tagtable": "Image::ExifTool::Any::ImageFormat", "start": "", "validate": False, "processproc": False}}, {},
        )
        self.assertTrue(any("edge target" in why for _key, why in moved.keyed_fact_mismatches))
        self.assertTrue(any("native facts" in why for _key, why in moved.keyed_fact_mismatches))

    def test_live_keyed_scope_rejects_a_deleted_generated_table(self):
        empty = verify.ParsedKeyedRust({}, {}, set(), set(), {}, {}, {})
        key = ("Any", "Main", "2058")
        inv = verify.keyed_native_inventory(
            empty, verify.ParsedNativeOmissions(True, {}), {key: "CanonRawMakeModel"},
            {}, {}, {}, set(), set(), {}, {}, {("Any", "Main"): ("MakerNotes", "", "")}, {}, {}, {},
        )
        self.assertEqual(inv.missing, (key,))


if __name__ == "__main__":
    unittest.main()
