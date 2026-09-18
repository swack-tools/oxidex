"""Source-only checks for the keyed-directory schema compiler.

No Rust reader is linked here.  These tests establish that source mutations
change emitted facts and that unavailable ProcessCanonRaw condition context is
withheld before a future caller can accidentally invent a retry.
"""

import copy
import hashlib
import json
import os
import shutil
import subprocess
import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

import codegen
import conds
import keyed_directory
import verify
from test_native_reader_contract import snapshot
from test_word_directory import PERL, PINNED, REPO_ROOT, _capture, _reader_contract, fixture_processor


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


def word_doc(processor=None):
    process = processor or fixture_processor()
    return {
        "native_reader_contracts": {"unsigned16": snapshot()},
        "modules": {"Any": {"tables": {
            "Main": {"meta": {"PROCESS_PROC": PROC, "GROUPS": {"0": "MakerNotes"}}, "tags": {
                "0x1001": {"Name": "Child", "SubDirectory": {
                    "TagTable": "Image::ExifTool::Any::Word",
                }},
            }},
            "Word": {"meta": {"PROCESS_PROC": process, "GROUPS": {"0": "MakerNotes", "2": "Camera"}}, "tags": {
                "1": {"Name": "One", "PrintConv": {0: "Off", 1: "On"}},
                "2": {"Name": "Two", "Condition": "$$self{Model} =~ /WORD/", "PrintConv": {0: "No", 1: "Yes"}},
            }},
            "Empty": {"meta": {"PROCESS_PROC": process, "GROUPS": {"0": "MakerNotes"}}, "tags": {}},
        }}},
    }


class Selection(unittest.TestCase):
    def test_selection_is_processor_identity_not_module_or_table(self):
        self.assertTrue(keyed_directory.is_keyed_directory_table({"PROCESS_PROC": PROC}))
        self.assertFalse(keyed_directory.is_keyed_directory_table({"PROCESS_PROC": BIN}))
        self.assertFalse(keyed_directory.is_keyed_directory_table({}))

    def test_dump_identity_shape_is_accepted(self):
        # This is the shape dump_tables.pl preserves for CanonRaw::Main.
        self.assertEqual(keyed_directory.processor_name({"PROCESS_PROC": PROC}), PROC["__name"])

    def test_word_layout_is_recognized_from_body_and_keeps_zero_row_table(self):
        source, stats = keyed_directory.generate(word_doc())
        self.assertIn("KEYED_ANY_WORD", source)
        self.assertIn("KEYED_ANY_EMPTY", source)
        self.assertIn("KeyedLayout::LengthPrefixedU16Pairs(WordDirectory {", source)
        self.assertIn("key_shift: 8", source)
        self.assertIn("value_format: Fmt::Int8u", source)
        self.assertIn('name: "One"', source)
        self.assertIn('name: "Two"', source)
        # The parent edge sees the generated target by identity, so its
        # processor blocker disappears without a module/table allowlist.
        parent = source.split("KEYED_ANY_WORD", 1)[0]
        self.assertNotIn('"target_processor"', parent)
        self.assertFalse(stats["keyed_word_processor"])

    def test_word_layout_keeps_a_wide_native_mask_as_numeric_descriptor_data(self):
        process = fixture_processor()
        process["__deparse"] = process["__deparse"].replace("& 255", "& 511", 1)
        source, _ = keyed_directory.generate(word_doc(process))
        self.assertIn("value_mask: 511", source)
        self.assertIn("value_format: Fmt::Int8u", source)

    def test_word_candidate_without_authenticated_facts_is_counted_and_withheld(self):
        process = fixture_processor()
        process["resolved"] = False
        source, stats = keyed_directory.generate(word_doc(process))
        self.assertNotIn("KEYED_ANY_WORD", source)
        self.assertNotIn("KEYED_ANY_EMPTY", source)
        self.assertEqual(stats["keyed_word_processor"], 2)
        # The Ciff parent stays visible, with its target plainly unwalked.
        self.assertIn('"target_processor"', source)

    def test_word_target_with_gate_a_refusal_remains_explicitly_unwalked(self):
        for name, mutation in (
            ("value metadata", lambda tag: tag.update(Format="int16u")),
            ("subdirectory", lambda tag: tag.update(SubDirectory={"TagTable": "Image::ExifTool::Any::Other"})),
        ):
            native = word_doc()
            mutation(native["modules"]["Any"]["tables"]["Word"]["tags"]["1"])
            source, stats = keyed_directory.generate(native)
            parent = source.split("KEYED_ANY_WORD", 1)[0]
            word = source.split("KEYED_ANY_WORD", 1)[1].split("\npub static ", 1)[0]
            with self.subTest(name=name):
                self.assertIn('"target_gate_a"', parent)
                self.assertNotIn('"target_processor"', parent)
                self.assertIn('gate_a: GateA { blocked_by: &[', word)
                self.assertGreater(stats["keyed_edge_unwalked"], 0)

    def test_existing_binary_target_remains_supported_alongside_word_layouts(self):
        source, stats = keyed_directory.generate(doc({
            "0x1001": {"Name": "MakeModel", "SubDirectory": {
                "TagTable": "Image::ExifTool::Any::MakeModel",
            }},
        }))
        parent = source.split("KEYED_ANY_MAIN", 1)[1].split("\npub static ", 1)[0]
        self.assertIn('table: "MakeModel"', parent)
        self.assertNotIn('"target_processor"', parent)
        self.assertFalse(stats["keyed_edge_unwalked"])


@unittest.skipUnless(PINNED, "set OXIDEX_PINNED_EXIFTOOL for native word-table generation")
class PinnedWordTableGeneration(unittest.TestCase):
    """Full source replay for CanonCustom's authenticated word processor.

    The dump must include Canon's binary targets as well as CanonRaw and
    CanonCustom.  Otherwise a valid CanonRaw parent edge looks unwalkable
    merely because its target table was excluded from the input.  The injected
    fact is captured from the actual native CV, including its package-local
    Get16u binding.  It models the post-capture shape that the dump pipeline
    carries; no Rust artifact is hand-authored here.
    """

    TABLE_ROWS = {
        "Functions10D": 17,
        "Functions1D": 22,
        "Functions20D": 18,
        "Functions30D": 19,
        "Functions350D": 9,
        "Functions400D": 11,
        "Functions5D": 21,
        "FunctionsD30": 15,
        "FuncsUnknown": 0,
    }

    def setUp(self):
        recorded_dump = os.environ.get("OXIDEX_TABLES_JSON")
        if recorded_dump:
            self.document = json.loads(Path(recorded_dump).read_text())
        else:
            result = subprocess.run(
                [PERL, str(REPO_ROOT / "tools" / "exiftool-tables" / "dump_tables.pl"),
                 str(Path(PINNED) / "lib")],
                check=True, text=True, capture_output=True,
            )
            self.document = json.loads(result.stdout)
        process = _capture(PINNED)
        captured = self.document["modules"]["CanonCustom"]["tables"]
        if recorded_dump:
            recorded = captured["Functions10D"]["meta"]["PROCESS_PROC"]
            self.assertTrue(recorded.get("resolved"))
            self.assertEqual(recorded.get("__name"), process["__name"])
            self.assertEqual(recorded.get("source_file"), process["source_file"])
            self.assertEqual(recorded.get("source_sha256"), process["source_sha256"])
        captured_tables = []
        for table, data in captured.items():
            meta = data.get("meta") or {}
            if meta.get("PROCESS_PROC", {}).get("__name") == process["__name"]:
                meta["PROCESS_PROC"] = copy.deepcopy(process)
                captured_tables.append(table)
        self.assertEqual(set(captured_tables), set(self.TABLE_ROWS))
        self.document["native_reader_contracts"] = {"unsigned16": _reader_contract(PINNED)}
        self.source, self.stats = keyed_directory.generate(self.document)

    def _table(self, table):
        symbol = f"KEYED_CANONCUSTOM_{table.upper()}"
        start = self.source.index(symbol)
        stop = self.source.find("\npub static ", start + 1)
        return self.source[start:] if stop < 0 else self.source[start:stop]

    def test_actual_source_generates_target_tables_and_zero_row_table(self):
        self.assertEqual(self.source.count("KeyedLayout::LengthPrefixedU16Pairs(WordDirectory {"),
                         len(self.TABLE_ROWS))
        for table, rows in self.TABLE_ROWS.items():
            with self.subTest(table=table):
                generated = self._table(table)
                self.assertIn("KeyedLayout::LengthPrefixedU16Pairs(WordDirectory {", generated)
                self.assertEqual(generated.count('name: "'), rows)
        self.assertIn('KEYED_CANONCUSTOM_FUNCSUNKNOWN', self.source)
        self.assertEqual(sum(self.TABLE_ROWS.values()), 132)

    def test_parent_processor_blocker_clears_only_after_authenticated_target_generation(self):
        parent = self._table("Main") if "KEYED_CANONCUSTOM_MAIN" in self.source else self.source
        # CanonRaw::Main is generated after CanonCustom in sorted module order.
        parent_start = self.source.index("KEYED_CANONRAW_MAIN")
        parent_stop = self.source.find("\npub static ", parent_start + 1)
        parent = self.source[parent_start:] if parent_stop < 0 else self.source[parent_start:parent_stop]
        for name in ("CustomFunctions10D", "CustomFunctionsD30", "CustomFunctionsD60", "CustomFunctionsUnknown"):
            field = parent[parent.index(f'name: "{name}"'):]
            next_field = field.find('KeyedTag {', len('KeyedTag {'))
            field = field if next_field < 0 else field[:next_field]
            self.assertNotIn('"target_processor"', field)
        # The complete source input leaves one CanonRaw child processor
        # explicit.  It is ProcessSerialData, not a failed word-table capture.
        af_info = parent[parent.index('name: "CanonAFInfo"'):]
        next_field = af_info.find('KeyedTag {', len('KeyedTag {'))
        af_info = af_info if next_field < 0 else af_info[:next_field]
        self.assertIn('module: "Canon", table: "AFInfo"', af_info)
        self.assertIn('unwalked: &["target_processor"]', af_info)
        self.assertEqual(parent.count('"target_processor"'), 1)
        self.assertEqual(self.stats["keyed_edge_unwalked"], 1)


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
    def test_unknown_fallback_preserves_known_alternatives_and_source_order(self):
        source, stats = keyed_directory.generate(doc({
            "0x180b": {"_variants": [
                {"Name": "SerialNumber", "Condition": "$$self{Model} =~ /EOS D30\\b/"},
                {"Name": "SerialNumber", "Condition": "$$self{Model} =~ /EOS/"},
                {"Name": "UnknownNumber", "Flags": "Unknown"},
            ]},
        }))
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "keyed.rs"
            path.write_text(source)
            parsed = verify.parse_keyed_rust(path)
            keys = [("Any", "Main", f"6155#{n}") for n in range(3)]
            self.assertEqual([parsed.fields[k] for k in keys],
                             ["SerialNumber", "SerialNumber", "UnknownNumber"])
            self.assertEqual([parsed.facts[k].flags[0] for k in keys], [False, False, True])
            self.assertEqual(verify.parse_omitted_keyed_native_rows(path).rows, {})
        self.assertFalse(stats["keyed_variant"])

    def test_flags_reuse_shared_policy_and_table_precedence(self):
        native = doc({
            "0x1001": {"Name": "A", "Unknown": 1, "Flags": {"Unknown": 0, "Binary": 1, "Priority": -2}},
            "0x1002": {"Name": "B", "Flags": ["List", "Protected", "Avoid"]},
        })
        native["modules"]["Any"]["tables"]["Main"]["meta"].update(AVOID=0, PRIORITY=3)
        source, _ = keyed_directory.generate(native)
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "keyed.rs"
            path.write_text(source)
            parsed = verify.parse_keyed_rust(path)
            self.assertEqual(parsed.facts[("Any", "Main", "4097")].flags,
                             (False, True, False, False, False, -2))
            self.assertEqual(parsed.facts[("Any", "Main", "4098")].flags,
                             (False, False, True, True, False, 3))

    def test_invalid_reporting_policy_refuses_generation(self):
        for extra in ({"Flags": [7]}, {"Priority": "many"}, {"Priority": 1 << 63}):
            with self.subTest(extra=extra), self.assertRaises(ValueError):
                keyed_directory.generate(doc({"0x1001": {"Name": "Mode", **extra}}))

    def test_falsy_flags_do_not_expand(self):
        tag = {"Name": "Mode", "Unknown": 1}
        plain, _ = keyed_directory.generate(doc({"0x1001": tag}))
        for flags in (None, "", "0", 0, False):
            with self.subTest(flags=flags):
                source, _ = keyed_directory.generate(doc({"0x1001": {**tag, "Flags": flags}}))
                self.assertEqual(source, plain)

    def test_flags_expand_once_before_variant_selection_and_omission_facts(self):
        condition = "$$self{Model} =~ /EOS/"
        changed = {
            "Name": "OldName", "Condition": "$$self{Model} =~ /D30/",
            "Flags": {"Name": "ExpandedName", "Condition": condition,
                      "Format": "int16u", "Count": 2,
                      "Flags": {"Name": "MustNotExpandTwice"}},
        }
        source, _ = keyed_directory.generate(doc({"0x180b": {"_variants": [changed]}}))
        self.assertIn(f"({conds.compile_cond(condition)}, KeyedTag", source)
        self.assertNotIn("OldName", source)
        self.assertNotIn("MustNotExpandTwice", source)
        self.assertIn('name: "ExpandedName"', source)
        refused = dict(changed)
        refused["Flags"] = {**changed["Flags"], "Format": "int16u[2]"}
        omitted, _ = keyed_directory.generate(doc({"0x180b": {"_variants": [refused]}}))
        self.assertIn('name: Some("ExpandedName")', omitted)
        self.assertNotIn("OldName", omitted)
        self.assertNotIn("MustNotExpandTwice", omitted)

    def test_defined_zero_priority_and_undefined_fallback(self):
        native = doc({
            "0x1001": {"Name": "Zero", "Priority": 0},
            "0x1002": {"Name": "Fallback", "Priority": None},
        })
        native["modules"]["Any"]["tables"]["Main"]["meta"]["PRIORITY"] = -1
        source, _ = keyed_directory.generate(native)
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "keyed.rs"
            path.write_text(source)
            parsed = verify.parse_keyed_rust(path)
            self.assertEqual(parsed.facts[("Any", "Main", "4097")].flags[-1], 0)
            self.assertEqual(parsed.facts[("Any", "Main", "4098")].flags[-1], -1)

    def test_native_keyed_processor_ignores_binary_processor_masks(self):
        # ProcessCanonRaw has no mask operation between ReadValue and FoundTag.
        # A copied native-source fixture confirms inline 3 stays 3 with Mask=1.
        plain, _ = keyed_directory.generate(doc({"0x100a": {"Name": "Mode"}}))
        for mask in (1, "not-a-binary-mask"):
            masked, _ = keyed_directory.generate(doc({"0x100a": {"Name": "Mode", "Mask": mask}}))
            self.assertEqual(masked, plain)

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

    def test_keyed_string_is_counted_bytes_and_sized_formats_are_refused(self):
        src, stats = keyed_directory.generate(doc({
            "0x0801": {"Name": "ThreeBytes", "Format": "string", "Count": 3},
            "0x1005": {"Name": "Sized", "Format": "int16u[3]"},
        }))
        self.assertIn('raw_id: 0x0801, name: "ThreeBytes", format: Some(Fmt::Str(1)), count: Some(3)', src)
        self.assertIn('raw_id: "0x1005", variant: false, name: Some("Sized"), native:', src)
        self.assertIn('reasons: &["format"]', src)
        self.assertEqual(stats["keyed_format"], 1)

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


class SharedExprRegistry(unittest.TestCase):
    def test_keyed_only_oracle_expression_is_declared_before_optional_output(self):
        # A keyed directory can be the only source of an oracle-approved ExprId.
        # The CLI must collect it before writing the shared binary enum, whether
        # or not the optional keyed artifact is requested.
        expression = "$val / 10"
        proc = {"__perl": "CODE", "__name": "Image::ExifTool::CanonRaw::ProcessCanonRaw"}
        source = {
            "exiftool_version": "13.59",
            "modules": {"Keyed": {"tables": {"Main": {
                "meta": {"PROCESS_PROC": proc},
                "tags": {"0x1001": {
                    "Name": "Scaled",
                    "PrintConv": {"kind": "expr", "expr": expression},
                }},
            }}}},
        }
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            tables = root / "tables.json"
            tables.write_text(json.dumps(source), encoding="utf-8")
            ledger = root / "ledger.json"
            ledger.write_text(json.dumps({
                "schema": codegen.LEDGER_SCHEMA,
                "exiftool_version": "13.59",
                "perl_version": "v5.38.2",
                "tables_sha256": hashlib.sha256(tables.read_bytes()).hexdigest(),
                "probe_counts": {"pass": 1, "fail": 0, "skip": 0},
                "verified_expressions": [codegen.exprs.normalize(expression)],
            }), encoding="utf-8")
            # `-o` names a mod.rs hub (one file per ExifTool module beside it).
            first = root / "first" / "mod.rs"
            second = root / "second" / "mod.rs"
            keyed = root / "keyed.rs"
            common = [sys.executable, str(Path(codegen.__file__)), str(tables),
                      "--expr-ledger", str(ledger)]
            subprocess.run([*common, "-o", str(first)], check=True, text=True,
                           capture_output=True)
            subprocess.run([*common, "-o", str(second), "--keyed-out", str(keyed)],
                           check=True, text=True, capture_output=True)
            ident = codegen.expr_ident(expression)
            self.assertIn(f"ExprId::{ident}", first.read_text(encoding="utf-8"))
            self.assertIn(f"ExprId::{ident}", second.read_text(encoding="utf-8"))
            self.assertEqual(first.read_text(encoding="utf-8"), second.read_text(encoding="utf-8"))
            self.assertIn(f"PrintConv::Expr(ExprId::{ident})", keyed.read_text(encoding="utf-8"))


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

    def test_rustfmt_preserves_independently_parsed_edge_and_native_facts(self):
        if shutil.which("rustfmt") is None:
            self.skipTest("rustfmt unavailable")
        path = self._artifact({
            "0x080a": {"Name": "CanonRawMakeModel", "SubDirectory": {
                "TagTable": "Image::ExifTool::Any::MakeModel",
            }},
            "0x1005": {"Name": "Sized", "Format": "int16u[3]"},
        })
        before = verify.parse_keyed_rust(path)
        omissions_before = verify.parse_omitted_keyed_native_rows(path)
        formatted = path.with_name("keyed_tables_rustfmt.rs")
        formatted.write_text(path.read_text(encoding="utf-8"), encoding="utf-8")
        subprocess.run(["rustfmt", "--edition", "2024", str(formatted)], check=True)
        self.assertEqual(verify.parse_keyed_rust(formatted), before)
        self.assertEqual(verify.parse_omitted_keyed_native_rows(formatted), omissions_before)
        malformed = formatted.with_name("keyed_tables_bad_edge.rs")
        malformed.write_text(
            formatted.read_text(encoding="utf-8").replace(
                "KeyedEdge::BoundedValue", "KeyedEdge::Unknown", 1,
            ),
            encoding="utf-8",
        )
        with self.assertRaises(SystemExit):
            verify.parse_keyed_rust(malformed)

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

    def test_native_flags_are_mandatory_and_reject_executable_or_source_drift(self):
        path = self._artifact({"0x1001": {"Name": "Mode", "Flags": ["Binary", "Unknown"]}})
        native = "\n".join([
            "KEYED\tAny\tMain\t\tTGROUPS\tMakerNotes\t\t",
            "KEYED\tAny\tMain\t4097\tNAME\tMode",
            "KEYED\tAny\tMain\t4097\tFLAGS\t1\t1\t0\t0\t0\t-",
            "KEYED\tAny\tMain\t4097\tUNKNOWN\t1",
        ])
        source = verify.parse_keyed_oracle(native)
        def inventory(artifact, oracle=source):
            return verify.keyed_native_inventory(
                verify.parse_keyed_rust(artifact), verify.parse_omitted_keyed_native_rows(artifact), *oracle
            )
        self.assertEqual(inventory(path).keyed_fact_mismatches, ())
        drift = path.with_name("flags-drift.rs")
        drift.write_text(path.read_text().replace("unknown: true", "unknown: false", 1))
        self.assertTrue(any("reporting flags" in why for _, why in inventory(drift).keyed_fact_mismatches))
        mutated = verify.parse_keyed_oracle(native.replace("FLAGS\t1\t1", "FLAGS\t0\t1"))
        self.assertTrue(inventory(path, mutated).keyed_fact_mismatches)
        with self.assertRaises(SystemExit):
            verify.parse_keyed_oracle("\n".join(line for line in native.splitlines() if "\tFLAGS\t" not in line))

    def test_sized_format_omission_is_authenticated_by_keyed_native_format_rules(self):
        # ProcessCanonRaw refuses the sized spelling rather than borrowing
        # ProcessBinaryData's array-format normalization.  This is a real
        # compiler artifact parsed by the independent verifier, not a
        # hand-built omission record.
        path = self._artifact({"0x1005": {"Name": "Sized", "Format": "int16u[3]"}})
        generated = verify.parse_keyed_rust(path)
        omissions = verify.parse_omitted_keyed_native_rows(path)
        key = ("Any", "Main", "4101")
        source = ({key: "Sized"}, {}, {key: "int16u[3]"}, {}, set(), set(), {},
                  {}, {("Any", "Main"): ("MakerNotes", "", "")}, {}, {}, {})
        fresh = verify.keyed_native_inventory(generated, omissions, *source)
        self.assertEqual(fresh.accounted, 1)
        self.assertEqual(fresh.bad_reasons, ())
        # A source change that makes the format executable must not let a
        # stale `format` sidecar hide the row.
        changed = list(source)
        changed[2] = {key: "int16u"}
        stale = verify.keyed_native_inventory(generated, omissions, *changed)
        self.assertEqual(stale.missing, ())
        self.assertEqual(len(stale.bad_reasons), 1)
        self.assertTrue(any("native facts" in why for _key, why in stale.keyed_fact_mismatches))

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

    def test_inventory_rejects_enum_value_drift_generated_orphans_and_unknown_printconv(self):
        path = self._artifact({"0x1001": {"Name": "Mode", "PrintConv": {
            "kind": "enum", "map": {"0": "Off", "1": "On"}, "directives": {},
        }}})
        src = path.read_text()
        key = ("Any", "Main", "4097")
        oracle_args = ({key: "Mode"}, {key: {"0": "Off", "1": "On"}}, {}, {}, set(), set(), {},
                       {}, {("Any", "Main"): ("MakerNotes", "", "")}, {}, {}, {})
        drift = path.with_name("enum-drift.rs")
        drift.write_text(src.replace('"On"', '"Broken"'), encoding="utf-8")
        inv = verify.keyed_native_inventory(
            verify.parse_keyed_rust(drift), verify.parse_omitted_keyed_native_rows(drift), *oracle_args
        )
        self.assertTrue(any("enum '1'" in why for _key, why in inv.keyed_fact_mismatches))
        orphan = verify.keyed_native_inventory(
            verify.parse_keyed_rust(path), verify.parse_omitted_keyed_native_rows(path),
            {}, {}, {}, {}, set(), set(), {}, {}, {("Any", "Main"): ("MakerNotes", "", "")}, {}, {}, {},
        )
        self.assertTrue(any("no live native row" in why for _key, why in orphan.keyed_fact_mismatches))
        unknown = path.with_name("unknown-printconv.rs")
        unknown.write_text(src.replace("PrintConv::IntEnum", "PrintConv::Mystery"), encoding="utf-8")
        with self.assertRaises(SystemExit):
            verify.parse_keyed_rust(unknown)


if __name__ == "__main__":
    unittest.main()
