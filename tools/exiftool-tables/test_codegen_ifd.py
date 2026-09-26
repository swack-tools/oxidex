"""codegen.py's IFD-style table emission (slice I-1).

The contract is docs/superpowers/specs/2026-09-06-ifd-tables-design.md
section 2 and the Rust schema src/exiftool_tables/ifd_schema.rs; every rule
below cites the Exif.pm / ExifTool.pm line it transcribes where one exists.
Run with `python3 -m unittest discover -s tools/exiftool-tables -p 'test*.py'`.

The whole-dump class needs a `dump_tables.pl` JSON: `OXIDEX_TABLES_JSON`, or
regen.sh's cache path `target/exiftool-src/tables-<pin>.json`; it skips
(loudly, naming both) when neither exists, so CI's dump-less unittest stage
stays green while a developer with the i7 dump gets the census asserted.
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path

import codegen
import table_modules
import exprs

ROOT = Path(__file__).resolve().parents[2]


def _ctx():
    return codegen.IfdGenContext(
        ifd_tables={("Olympus", "Equipment"), ("Olympus", "Main")},
        binary_tables={("Minolta", "CameraSettings")},
    )


def _emit(tag, tag_id=0x0203, stats=None, verified=None, meta=None, condition_resolved=False):
    stats = codegen.new_ifd_stats() if stats is None else stats
    src, reason = codegen.gen_ifd_tag_literal(
        tag, tag_id, stats, verified, _ctx(), meta or {}, condition_resolved=condition_resolved
    )
    return src, reason, stats


def _subdir(tag, stats=None):
    stats = codegen.new_ifd_stats() if stats is None else stats
    src = codegen.compile_ifd_subdir(codegen.expand_flags(tag, stats), stats, _ctx())
    return src, stats


def _plain(stats):
    """The scalar counters of `stats` that fired, for `assertEqual` against a
    dict -- the nested diagnostic Counters are dropped."""
    return {k: v for k, v in stats.items() if not isinstance(v, Counter) and v}


class Selection(unittest.TestCase):
    def test_process_proc_absent_is_an_ifd_table(self):
        # ExifTool.pm:9052 defaults PROCESS_PROC to Exif::ProcessExif.
        self.assertTrue(codegen.is_ifd_table({}))
        self.assertTrue(codegen.is_ifd_table({"GROUPS": {"0": "MakerNotes"}}))

    def test_explicit_process_exif_is_an_ifd_table(self):
        pp = {"__perl": "CODE", "__name": "Image::ExifTool::Exif::ProcessExif"}
        self.assertTrue(codegen.is_ifd_table({"PROCESS_PROC": pp}))
        self.assertTrue(codegen.is_ifd_table({"PROCESS_PROC": "Image::ExifTool::Exif::ProcessExif"}))

    def test_process_binary_data_and_others_are_not(self):
        for name in ("Image::ExifTool::ProcessBinaryData", "Image::ExifTool::Sony::ProcessEnciphered",
                     "Image::ExifTool::Kodak::ProcessKodakIFD"):
            meta = {"PROCESS_PROC": {"__perl": "CODE", "__name": name}}
            self.assertFalse(codegen.is_ifd_table(meta), name)
            self.assertEqual(
                codegen.is_binary_table(meta), name.endswith("ProcessBinaryData"), name
            )
        self.assertFalse(codegen.is_ifd_table({"PROCESS_PROC": 7}))


class TagIds(unittest.TestCase):
    def test_decimal_keys_in_the_16_bit_range(self):
        self.assertEqual(codegen.parse_ifd_tag_id("515"), 0x0203)
        self.assertEqual(codegen.parse_ifd_tag_id("0"), 0)
        self.assertEqual(codegen.parse_ifd_tag_id("65535"), 0xFFFF)

    def test_everything_else_is_refused(self):
        for key in ("65536", "0x0203", "ANNO", "-1", "0515", "1.1", "", "Duration", "0002,0000"):
            self.assertIsNone(codegen.parse_ifd_tag_id(key), key)

    def test_refused_keys_are_counted_and_block_gate_a(self):
        tbl = {"meta": {}, "tags": {"ANNO": {"Name": "Annotation"}, "5": {"Name": "Five"}}}
        run = codegen.new_ifd_stats()
        src = codegen.gen_ifd_table("AIFF", "Main", tbl, run, None, _ctx())
        self.assertEqual(run["ifd_tag_id_unrepresentable"], 1)
        self.assertEqual(run["ifd_tag_emitted"], 1)
        self.assertIn('gate_a: GateA { blocked_by: &[("ifd_tag_id_unrepresentable", 1)] }', src)
        self.assertEqual(run["ifd_gate_a_fail"], 1)


class IdentityLedger(unittest.TestCase):
    def _doc(self):
        return {
            "exiftool_version": "13.59",
            "modules": {"Test": {"tables": {"Main": {"meta": {}, "tags": {
                "1": {"Name": "Alpha", "Format": "int16u", "RawConv": "$val"},
                "2": {"Name": "NoFormat", "Format": "unmodelled"},
                "raw": {"Name": "NotAnIfdId"},
                "3": {"_variants": [
                    {"Name": "First", "Format": "int16u", "Condition": '$format eq "int16u"'},
                    {"Name": "Second", "Format": "int16u"},
                ]},
            }}}}},
        }

    def test_compiler_records_exact_rows_and_refusals_without_changing_rust(self):
        doc = self._doc()
        ctx = codegen.IfdGenContext.from_doc(doc)
        plain = codegen.gen_ifd_table(
            "Test", "Main", doc["modules"]["Test"]["tables"]["Main"],
            codegen.new_ifd_stats(), None, ctx,
        )
        emitted, rows = codegen.gen_ifd_table(
            "Test", "Main", doc["modules"]["Test"]["tables"]["Main"],
            codegen.new_ifd_stats(), None, ctx, include_identity_ledger=True,
        )
        self.assertEqual(emitted, plain, "ledger collection must not alter Rust")
        by_identity = {(row["raw_key"], tuple(row["variant_path"])): row for row in rows}
        self.assertEqual(by_identity[("1", ())]["state"], "emitted")
        self.assertEqual(by_identity[("1", ())]["reader_state"], "omitted")
        self.assertEqual(by_identity[("1", ())]["omissions"], ["raw_conv"])
        self.assertEqual(by_identity[("2", ())]["reasons"], ["ifd_format_unsupported"])
        self.assertEqual(by_identity[("raw", ())]["reasons"], ["raw_key_unrepresentable"])
        self.assertEqual(by_identity[("3", (0,))]["state"], "emitted")
        self.assertEqual(by_identity[("3", (1,))]["name"], "Second")
        self.assertRegex(by_identity[("1", ())]["source_sha256"], r"^[0-9a-f]{64}$")
        changed = dict(doc["modules"]["Test"]["tables"]["Main"]["tags"]["1"])
        changed["Name"] = "Renamed"
        self.assertNotEqual(
            by_identity[("1", ())]["source_sha256"],
            codegen._ifd_identity_source_sha256(changed),
        )
        refused_doc = json.loads(json.dumps(doc))
        refused_doc["modules"]["Test"]["tables"]["Main"]["tags"]["3"]["_variants"][0]["Condition"] = "unsupported($val)"
        _, refused_rows = codegen.gen_ifd_table(
            "Test", "Main", refused_doc["modules"]["Test"]["tables"]["Main"],
            codegen.new_ifd_stats(), None, codegen.IfdGenContext.from_doc(refused_doc),
            include_identity_ledger=True,
        )
        self.assertEqual(
            {(row["raw_key"], tuple(row["variant_path"])): row for row in refused_rows}[("3", (0,))]["reasons"],
            ["tag_variant_cond_unsupported"],
        )
        raw_variants = {"meta": {}, "tags": {"not-a-tag-id": {"_variants": [
            {"Name": "One"}, {"Name": "Two"},
        ]}}}
        _, raw_rows = codegen.gen_ifd_table(
            "Test", "Raw", raw_variants, codegen.new_ifd_stats(), None, _ctx(),
            include_identity_ledger=True,
        )
        self.assertEqual(
            {(row["raw_key"], tuple(row["variant_path"]), row["name"]) for row in raw_rows},
            {("not-a-tag-id", (0,), "One"), ("not-a-tag-id", (1,), "Two")},
        )

    def test_binary_ownership_rows_preserve_fractional_and_kodak_raw_keys(self):
        binary = {"__name": "Image::ExifTool::ProcessBinaryData"}
        doc = {
            "modules": {
                "Nikon": {"tables": {"MakerNotes0x56": {
                    "meta": {"PROCESS_PROC": binary},
                    "tags": {
                        "4.1": {"Name": "BurstStartSlotNumber"},
                        "4.4": {"Name": "BurstStartImageType"},
                        "PRINT_CONV": {"Name": "not a field"},
                        "Groups": {},
                    },
                }}},
                "Kodak": {"tables": {"Main": {
                    "meta": {"PROCESS_PROC": binary},
                    "tags": {"20": {"Name": "TimeCreated"}},
                }}},
                "JPEG": {"tables": {"MediaJukebox": {
                    "meta": {"VARS": {"ID_FMT": "none"}},
                    "tags": {
                        "": {},
                        "123": {},
                        "Album": {},
                        "Caption": {},
                        "Date": {},
                        "Explicit": {"Name": "ExplicitName"},
                        "Keywords": {},
                        "Name": {},
                        "People": {},
                        "Places": {},
                        "Tool_Name": {},
                        "Tool_Version": {},
                    },
                }}},
                "Lookup": {"tables": {"Values": {
                    "meta": {},
                    "tags": {"OTHER": {}},
                }, "ValuesWithIdFormat": {
                    "meta": {"VARS": {"ID_FMT": "hex"}},
                    "tags": {"OTHER": {}},
                }}},
                "Trailer": {"tables": {"Vivo": {
                    "meta": {"VARS": {"ID_FMT": "none"}},
                    "tags": {"HDRImage": {}, "HiddenData": {}, "JSONInfo": {}},
                }}},
                "Samsung": {"tables": {"Trailer": {
                    "meta": {"PROCESS_PROC": {"__name": "Image::ExifTool::Samsung::ProcessSamsung"}, "VARS": {"ID_FMT": "none"}},
                    "tags": {
                        "0x0001": {"Name": "EmbeddedImage"},
                        "0x0100-name": {"Name": "EmbeddedAudioFileName", "_shorthand": True},
                        "0x0100": {"Name": "EmbeddedAudioFile", "Binary": "1"},
                        "0x0201": {"Name": "SurroundShotVideo"},
                    },
                }}},
            },
        }
        rows = codegen.gen_ownership_identities(
            doc, ["JPEG", "Kodak", "Lookup", "Nikon", "Samsung", "Trailer"]
        )
        by_identity = {
            (row["full_name"], row["raw_key"], tuple(row["variant_path"])): row
            for row in rows
        }
        self.assertEqual(
            by_identity[("Image::ExifTool::Nikon::MakerNotes0x56", "4.1", ())]["name"],
            "BurstStartSlotNumber",
        )
        self.assertEqual(
            by_identity[("Image::ExifTool::Nikon::MakerNotes0x56", "4.4", ())]["name"],
            "BurstStartImageType",
        )
        self.assertEqual(
            by_identity[("Image::ExifTool::Kodak::Main", "20", ())]["name"],
            "TimeCreated",
        )
        self.assertNotIn(
            ("Image::ExifTool::Nikon::MakerNotes0x56", "PRINT_CONV", ()),
            by_identity,
        )
        self.assertNotIn(
            ("Image::ExifTool::Nikon::MakerNotes0x56", "Groups", ()),
            by_identity,
        )
        self.assertEqual(
            {
                raw_key
                for full_name, raw_key, variant_path in by_identity
                if full_name == "Image::ExifTool::JPEG::MediaJukebox"
                and not variant_path
            },
            {
                "Album", "Caption", "Date", "Keywords", "Name", "People",
                "Places", "Tool_Name", "Tool_Version",
            },
        )
        self.assertEqual(
            {
                raw_key
                for full_name, raw_key, variant_path in by_identity
                if full_name == "Image::ExifTool::Trailer::Vivo" and not variant_path
            },
            {"HDRImage", "HiddenData", "JSONInfo"},
        )
        self.assertNotIn(
            ("Image::ExifTool::Lookup::Values", "OTHER", ()),
            by_identity,
        )
        self.assertEqual(
            {
                (raw_key, name)
                for full_name, raw_key, variant_path in by_identity
                if full_name == "Image::ExifTool::Samsung::Trailer" and not variant_path
                for name in [by_identity[(full_name, raw_key, variant_path)]["name"]]
            },
            {("0x0100-name", "EmbeddedAudioFileName"), ("0x0100", "EmbeddedAudioFile")},
        )
        self.assertNotIn(
            ("Image::ExifTool::Lookup::ValuesWithIdFormat", "OTHER", ()),
            by_identity,
        )
        for raw_key in ("", "123", "Explicit"):
            self.assertNotIn(
                ("Image::ExifTool::JPEG::MediaJukebox", raw_key, ()),
                by_identity,
            )
        self.assertEqual(
            {row["source_kind"] for row in rows},
            {"binary", "named-raw-key", "samsung-trailer"},
        )
        self.assertEqual(
            codegen.ownership_identity_counts(rows),
            {"binary_rows": 3, "named_raw_key_rows": 12, "samsung_trailer_rows": 2},
        )
        self.assertTrue(all(re.fullmatch(r"[0-9a-f]{64}", row["source_sha256"]) for row in rows))
        self.assertFalse(any(
            row["full_name"] == "Image::ExifTool::Samsung::Trailer"
            for row in codegen.gen_ownership_identities(doc, ["JPEG", "Trailer"])
        ))

    def test_cli_binds_ledger_to_input_and_emitted_ifd_rust(self):
        doc = self._doc()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tables, binary, ifd, ledger = (root / name for name in ("tables.json", "binary/mod.rs", "ifd/mod.rs", "ledger.json"))
            tables.write_text(json.dumps(doc))
            result = subprocess.run([
                sys.executable, str(ROOT / "tools" / "exiftool-tables" / "codegen.py"), str(tables),
                "-o", str(binary), "--ifd-out", str(ifd), "--ifd-identity-ledger-out", str(ledger),
            ], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(ledger.read_text())
            self.assertEqual(report["schema"], "oxidex_ifd_identity_ledger_v1")
            self.assertIsNone(report["source"]["expr_ledger_sha256"])
            self.assertEqual(report["counts"], {"rows": 5, "emitted": 3, "refused": 2,
                                                "reader_eligible": 2, "reader_omitted": 1})
            self.assertEqual(
                report["ownership_counts"],
                {
                    "binary_rows": 0,
                    "named_raw_key_rows": 0,
                    "samsung_trailer_rows": 0,
                },
            )
            self.assertEqual(report["ownership_rows"], [])
            self.assertEqual(
                report["source"]["tables_json_sha256"],
                __import__("hashlib").sha256(tables.read_bytes()).hexdigest(),
            )
            self.assertEqual(report["source"]["ifd_rust_hash_format"], codegen.IFD_RUST_HASH_FORMAT)
            self.assertEqual(
                report["source"]["ifd_rust_sha256"],
                codegen._canonical_ifd_rust_sha256(table_modules.read_files(ifd)),
            )
            # One file per ExifTool module beside each hub, and the hub
            # re-exports each of them under its old path.
            self.assertEqual(sorted(p.name for p in ifd.parent.glob("*.rs")), ["mod.rs", "test.rs"])
            self.assertIn("pub use test::*;", ifd.read_text())
            self.assertIn("pub static IFD_TEST_MAIN:", (ifd.parent / "test.rs").read_text())
            self.assertNotIn("pub static IFD_", ifd.read_text())
            self.assertEqual(sorted(p.name for p in binary.parent.glob("*.rs")), ["mod.rs"])
            missing_ifd = subprocess.run([
                sys.executable, str(ROOT / "tools" / "exiftool-tables" / "codegen.py"), str(tables),
                "-o", str(binary), "--ifd-identity-ledger-out", str(ledger),
            ], text=True, capture_output=True)
            self.assertNotEqual(missing_ifd.returncode, 0)
            self.assertIn("requires --ifd-out", missing_ifd.stderr)


class FlagsAndTruthiness(unittest.TestCase):
    def test_perl_truthy(self):
        for falsy in (None, "", "0", 0, False):
            self.assertFalse(codegen.perl_truthy(falsy), repr(falsy))
        for truthy in ("1", "2", "00", 1, -1, "Off", [], {}):
            self.assertTrue(codegen.perl_truthy(truthy), repr(truthy))

    def test_expand_flags_is_setup_tag_tables_expansion(self):
        # ExifTool.pm:5877-5893: ARRAY -> each key = 1; scalar -> that key = 1;
        # HASH -> key/value.
        stats = Counter()
        t = codegen.expand_flags({"Name": "T", "Flags": ["Unknown", "Binary", "Drop"]}, stats)
        self.assertEqual((t["Unknown"], t["Binary"], t["Drop"]), (1, 1, 1))
        t = codegen.expand_flags({"Name": "T", "Flags": "SubIFD"}, stats)
        self.assertEqual(t["SubIFD"], 1)
        t = codegen.expand_flags({"Name": "T", "Flags": {"Priority": "0"}}, stats)
        self.assertEqual(t["Priority"], "0")
        self.assertEqual(stats["ifd_flags_unreadable"], 0)
        # A non-string flag cannot be named and is counted (Gate-A disqualifying).
        codegen.expand_flags({"Name": "T", "Flags": [7]}, stats)
        self.assertEqual(stats["ifd_flags_unreadable"], 1)
        # The input dict is never mutated.
        raw = {"Name": "T", "Flags": "List"}
        codegen.expand_flags(raw, stats)
        self.assertNotIn("List", raw)


class ValueDomain(unittest.TestCase):
    def test_domains(self):
        self.assertEqual(codegen.ifd_value_domain("int16u", None, True), "num")
        self.assertEqual(codegen.ifd_value_domain("int16u", 1, True), "num")
        self.assertEqual(codegen.ifd_value_domain("rational64u", 3, True), "list")
        # ReadValue returns one byte string whatever the count.
        self.assertEqual(codegen.ifd_value_domain("string", 11, True), "str")
        self.assertEqual(codegen.ifd_value_domain("undef", None, True), "bytes")
        # Unknown count (Count => -1) or no spelling at all: no domain.
        self.assertIsNone(codegen.ifd_value_domain("int16u", None, False))
        self.assertIsNone(codegen.ifd_value_domain(None, None, True))
        self.assertIsNone(codegen.ifd_value_domain("lang-alt", None, True))


class TagLiteral(unittest.TestCase):
    def test_olympus_bwmode_shape(self):
        # Olympus.pm 0x0203 exactly as the dump carries it.
        tag = {"Name": "BWMode", "Writable": "int16u",
               "PrintConv": {"kind": "enum", "map": {"0": "Off", "1": "On", "6": "(none)"}, "directives": None}}
        src, reason, stats = _emit(tag)
        self.assertIsNone(reason)
        self.assertEqual(
            src,
            'IfdTag { id: 0x0203, name: "BWMode", format: None, count: None, '
            'writable: Some("int16u"), groups: TagGroups::NONE, flags: IfdFlags::NONE, '
            "condition: None, omitted: Omitted::NONE, raw_conv: None, value_conv: None, "
            'print_conv: PrintConv::IntEnum(&[(0, "Off"), (1, "On"), (6, "(none)")]), subdir: None }',
        )
        self.assertEqual(_plain(stats), {"enum_int": 1})

    def test_format_override_and_count(self):
        src, _, _ = _emit({"Name": "SensorArea", "Format": "int16u", "Count": "4", "Writable": "undef"})
        self.assertIn("format: Some(Fmt::Int16u), count: Some(4), writable: Some(\"undef\")", src)
        # `fmt[N]`: the format and the count both come from the spelling.
        src, _, stats = _emit({"Name": "FillPat", "Format": "int8u[8]"})
        self.assertIn("format: Some(Fmt::Int8u), count: Some(8)", src)
        src, _, _ = _emit({"Name": "T", "Format": "string[11]"})
        self.assertIn("format: Some(Fmt::Str(11)), count: Some(11)", src)
        # Count alone is data.
        src, _, _ = _emit({"Name": "SpecialMode", "Count": "3", "Writable": "int32u"})
        self.assertIn("format: None, count: Some(3), writable: Some(\"int32u\")", src)

    def test_bare_string_and_undef_formats_emit_the_unsized_override(self):
        # Exif.pm:6737-6745 makes the override live: the entry's bytes are
        # re-read as this kind with the entry's own length, which the schema
        # spells `Fmt::Str(0)` / `Fmt::Undef(0)` and the walk honours
        # (`ReadValue` NUL-truncates `string` only). Not a Gate-A hazard.
        src, reason, stats = _emit({"Name": "CameraID", "Format": "string"})
        self.assertIsNone(reason)
        self.assertIn("format: Some(Fmt::Str(0)), count: None, writable: None", src)
        src, reason, stats2 = _emit({"Name": "Blob", "Format": "undef", "Count": "4"})
        self.assertIsNone(reason)
        self.assertIn("format: Some(Fmt::Undef(0)), count: Some(4), writable: None", src)
        self.assertEqual(stats["ifd_format_unsized"], 1)
        self.assertEqual(stats2["ifd_format_unsized"], 1)
        self.assertNotIn("ifd_format_unsized", codegen.GATE_A_DISQUALIFYING)
        self.assertNotIn("ifd_format_unsized_override", codegen.GATE_A_DISQUALIFYING)

    def test_unsupported_format_refuses_the_tag(self):
        for spelling in ("Rect", "unsigned", "ifd", "string[0,32]", "digits[8]", "var_string", 7):
            src, reason, stats = _emit({"Name": "T", "Format": spelling})
            self.assertIsNone(src, spelling)
            self.assertEqual(reason, "ifd_format_unsupported", spelling)
            self.assertEqual(stats["ifd_format_unsupported"], 1, spelling)

    def test_count_minus_one_is_none_and_kills_the_domain(self):
        tag = {"Name": "SubjectArea", "Writable": "int16u", "Count": "-1",
               "PrintConv": {"kind": "expr", "expr": "$val / 10"}}
        src, _, stats = _emit(tag, verified={exprs.normalize("$val / 10")})
        self.assertIn("count: None", src)
        self.assertIn("print_conv: PrintConv::None", src)
        self.assertEqual(stats["ifd_count_unrepresentable"], 1)
        self.assertEqual(stats["ifd_expr_domain_unknown"], 1)

    def test_writable_spellings(self):
        src, _, stats = _emit({"Name": "T", "Writable": "rational64s"})
        self.assertIn('writable: Some("rational64s")', src)
        # An integer Writable is a permission, not a format: None, no counter.
        src, _, stats = _emit({"Name": "T", "Writable": "1"})
        self.assertIn("writable: None", src)
        self.assertEqual(stats["ifd_writable_unmodeled"], 0)
        src, _, stats = _emit({"Name": "T", "Writable": "lang-alt"})
        self.assertIn("writable: None", src)
        self.assertEqual(stats["ifd_writable_unmodeled"], 1)

    def test_expr_print_conv_takes_its_domain_from_writable(self):
        verified = {exprs.normalize("$val / 10")}
        tag = {"Name": "T", "Writable": "int16u", "PrintConv": {"kind": "expr", "expr": "$val / 10"}}
        src, _, stats = _emit(tag, verified=verified)
        self.assertIn(f"print_conv: PrintConv::Expr(ExprId::{codegen.expr_ident('$val / 10')})", src)
        self.assertEqual(stats["expr_compiled"] + stats["expr_translated"], 1)
        # Format wins over Writable when both are declared.
        tag = {"Name": "T", "Format": "int32u", "Writable": "string",
               "PrintConv": {"kind": "expr", "expr": "$val / 10"}}
        src, _, _ = _emit(tag, verified=verified)
        self.assertIn("PrintConv::Expr(ExprId::", src)
        # A str-domain tag must not run a num-domain expression -- and for an
        # IFD tag the refusal WITHHOLDS the tag (Omitted.print_conv) instead of
        # emitting it raw: the refusal is re-counted under the withheld
        # counters, never under the table-disqualifying key.
        tag = {"Name": "T", "Writable": "string", "PrintConv": {"kind": "expr", "expr": "$val / 10"}}
        src, _, stats = _emit(tag, verified=verified)
        self.assertIn("print_conv: PrintConv::None", src)
        self.assertIn("print_conv: true", src.split("omitted: Omitted {", 1)[1].split("}", 1)[0])
        self.assertEqual(stats["expr_refused_input_domain"], 0)
        self.assertEqual(stats["ifd_print_conv_withheld"], 1)
        self.assertEqual(stats["ifd_print_conv_withheld_by"]["expr_refused_input_domain"], 1)
        # No Format, no Writable: no domain at all -> ifd_expr_domain_unknown.
        tag = {"Name": "T", "PrintConv": {"kind": "expr", "expr": "$val / 10"},
               "ValueConv": {"kind": "expr", "expr": "$val / 10"}}
        src, _, stats = _emit(tag, verified=verified)
        self.assertIn("value_conv: None, print_conv: PrintConv::None", src)
        self.assertEqual(stats["ifd_expr_domain_unknown"], 2)
        # Both conversions are refused AND the field is withheld: honest
        # absence (Omitted flags), not a Gate-A hazard.
        self.assertIn("omitted: Omitted { value_conv: true,", src)
        self.assertIn("print_conv: true", src.split("omitted: Omitted {", 1)[1].split("}", 1)[0])
        self.assertNotIn("ifd_expr_domain_unknown", codegen.GATE_A_DISQUALIFYING)
        self.assertNotIn("ifd_bitmask_words_unsupported", codegen.GATE_A_DISQUALIFYING)
        # Enums need no domain.
        tag = {"Name": "T", "PrintConv": {"kind": "enum", "map": {"1": "On"}, "directives": None}}
        src, _, stats = _emit(tag)
        self.assertIn('PrintConv::IntEnum(&[(1, "On")])', src)
        self.assertEqual(stats["ifd_expr_domain_unknown"], 0)

    def test_flags_are_carried_including_via_flags_expansion(self):
        tag = {"Name": "T", "Flags": ["Unknown", "Binary", "Protected"], "List": "1", "Avoid": "1",
               "Priority": "0"}
        src, reason, stats = _emit(tag)
        self.assertIsNone(reason, "Unknown is a flag, never a drop")
        self.assertIn(
            "flags: IfdFlags { unknown: true, binary: true, list: true, protected: true, "
            "avoid: true, priority: Some(0) }",
            src,
        )
        self.assertEqual(stats["ifd_flag_unknown"], 1)
        src, _, _ = _emit({"Name": "T", "Unknown": "1"})
        self.assertIn("unknown: true, binary: false", src)
        src, _, _ = _emit({"Name": "T"})
        self.assertIn("flags: IfdFlags::NONE", src)

    def test_table_avoid_overrides_the_tags_own(self):
        # ExifTool.pm:5915 `$$tagInfo{Avoid} = $avoid if defined $avoid`.
        src, _, _ = _emit({"Name": "T"}, meta={"AVOID": "1"})
        self.assertIn("avoid: true", src)
        src, _, _ = _emit({"Name": "T", "Avoid": "1"}, meta={"AVOID": "0"})
        self.assertIn("flags: IfdFlags::NONE", src)

    def test_priority_not_an_integer_is_counted(self):
        _, _, stats = _emit({"Name": "T", "Priority": "high"})
        self.assertEqual(stats["ifd_priority_unreadable"], 1)

    def test_set_member_raw_conv_is_carried_as_data(self):
        for text in ("$$self{Foo} = $val", "$$self{Foo} = $val;", "  $$self{Foo}=$val ;",
                     "$self->{Foo} = $val"):
            tag = {"Name": "T", "Writable": "int16u", "DataMember": "Foo",
                   "RawConv": {"kind": "expr", "expr": text}}
            src, _, stats = _emit(tag)
            self.assertIn('raw_conv: Some(RawConvEffect::SetMember { member: "Foo" })', src, text)
            self.assertIn("omitted: Omitted::NONE", src, text)
            self.assertEqual(stats["ifd_raw_conv_set_member"], 1, text)
            self.assertEqual(stats["ifd_data_member_unmodeled"], 0, text)

    def test_every_other_raw_conv_is_omitted(self):
        for text in ("$val =~ s/\\s+$//; $$self{Make} = $val", "$val == 0x7fffffff ? undef : $val",
                     "$$self{TrackTypes}{$val} = 1; $$self{TrackType} = $val"):
            tag = {"Name": "T", "Writable": "string", "DataMember": "Make",
                   "RawConv": {"kind": "expr", "expr": text}}
            src, _, stats = _emit(tag)
            self.assertIn("raw_conv: None", src, text)
            self.assertIn("omitted: Omitted { value_conv: false, raw_conv: true,", src, text)
            self.assertEqual(stats["omitted_raw_conv"], 1, text)
            self.assertEqual(stats["ifd_data_member_unmodeled"], 1, text)
        # A CODE-ref RawConv likewise.
        src, _, _ = _emit({"Name": "T", "RawConv": {"kind": "code", "deparse": "..."}})
        self.assertIn("raw_conv: true", src)

    def test_offset_tags_are_refused_not_emitted(self):
        # Spec section 6 -- not Gate-A disqualifying.
        for tag in (
            {"Name": "PreviewImageStart", "Flags": "IsOffset"},
            {"Name": "PreviewImageLength", "_extra_keys": ["OffsetPair"]},
            {"Name": "PreviewImage", "_extra_keys": ["DataTag", "WriteCheck"]},
            {"Name": "PreviewImage", "ChangeBase": "$dirStart + $dataPos - 8"},
            {"Name": "StripOffsets", "_extra_keys": ["IsOffset"]},
        ):
            src, reason, stats = _emit(tag)
            self.assertIsNone(src, tag)
            self.assertEqual(reason, "ifd_isoffset_unsupported", tag)
            self.assertEqual(_plain(stats), {"ifd_isoffset_unsupported": 1}, tag)
        self.assertNotIn("ifd_isoffset_unsupported", codegen.GATE_A_DISQUALIFYING)

    def test_bitmask_with_bits_per_word_refuses_the_print_conv(self):
        tag = {"Name": "AFPointsUsed", "Writable": "int8u", "BitsPerWord": "8", "BitsTotal": "80",
               "PrintConv": {"kind": "enum_partial", "map": {}, "directives": {"BITMASK": {"0": "a"}}}}
        src, _, stats = _emit(tag)
        self.assertIn("print_conv: PrintConv::None", src)
        self.assertEqual(stats["ifd_bitmask_words_unsupported"], 1)
        self.assertEqual(stats["bitmask_emitted"], 0)

    def test_code_ref_print_conv_withholds_the_tag(self):
        tag = {"Name": "SpecialMode", "Writable": "int32u", "Count": "3",
               "PrintConv": {"kind": "code", "expr": None, "deparse": "{ package X; ... }"}}
        src, _, stats = _emit(tag)
        self.assertIn("subdirectory: false, print_conv: true }", src)
        self.assertEqual(stats["conv_dropped"], 1)

    def test_condition_on_a_single_tag_is_compiled_or_explicitly_omitted(self):
        tag = {"Name": "T", "Condition": '$$self{Model} =~ /E-M1/'}
        src, _, stats = _emit(tag)
        self.assertIn("condition: Some(Cond::MemberRegex", src)
        self.assertIn("omitted: Omitted::NONE", src)
        refused, _, _ = _emit({"Name": "T", "Condition": "unsupported($val)"})
        self.assertIn("condition: None", refused)
        self.assertIn("condition: true", refused)
        src, _, _ = _emit(tag, condition_resolved=True)
        self.assertIn("condition: None", src)
        self.assertIn("omitted: Omitted::NONE", src)

    def test_no_name_is_refused(self):
        src, reason, _ = _emit({"Writable": "int16u"})
        self.assertEqual((src, reason), (None, "tag_no_name"))
        src, reason, _ = _emit({"_unhandled": "SCALAR"})
        self.assertEqual(reason, "tag_no_name")

    def test_groups_are_the_existing_tag_groups_emission(self):
        src, _, stats = _emit({"Name": "T", "Groups": {"2": "Preview"}})
        self.assertIn('groups: TagGroups { g0: None, g1: None, g2: Some("Preview") }', src)
        self.assertEqual(stats["tag_group_override"], 1)


class SubdirEdges(unittest.TestCase):
    def _edge(self, sd=None, **tag_extra):
        tag = {"Name": "Equipment", "SubDirectory": sd or {"TagTable": "Image::ExifTool::Olympus::Equipment"}}
        tag.update(tag_extra)
        return _subdir(tag)

    def test_start_forms(self):
        cases = {
            None: "IfdStart::ValuePtr(0)",
            "$valuePtr": "IfdStart::ValuePtr(0)",
            "$valuePtr + 8": "IfdStart::ValuePtr(8)",
            "$valuePtr - 2": "IfdStart::ValuePtr(-2)",
            "$val": "IfdStart::Val(0)",
            "$val - 4": "IfdStart::Val(-4)",
            "$val + 20": "IfdStart::Val(20)",
        }
        for start, want in cases.items():
            self.assertEqual(codegen.compile_ifd_start(start), want, start)
            src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": start})
            self.assertIn(f"start: {want},", src, start)
        for bad in ("16", "$valuePtr * 2", "$dirStart + $val", "$val + $valuePtr", 4):
            src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": bad})
            self.assertEqual(src, "None", bad)
            self.assertEqual(_plain(stats), {"ifd_subdir_refused_start": 1}, bad)

    def test_byte_order_spellings_follow_exif_pm(self):
        # Exif.pm:6970-6972: /^Little/i, /^Big/i; only 'Unknown' detects.
        cases = {
            None: "IfdByteOrder::Inherit",
            "LittleEndian": "IfdByteOrder::Little",
            "Little-endian": "IfdByteOrder::Little",
            "littleendian": "IfdByteOrder::Little",
            "BigEndian": "IfdByteOrder::Big",
            "Unknown": "IfdByteOrder::Unknown",
        }
        for spelling, want in cases.items():
            self.assertEqual(codegen.compile_ifd_byte_order(spelling), want, spelling)
        for bad in ("II", "MM", "unknown", "Motorola", 1):
            src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "ByteOrder": bad})
            self.assertEqual(src, "None", bad)
            self.assertEqual(_plain(stats), {"ifd_subdir_refused_byteorder": 1}, bad)

    def test_fix_format_and_sub_ifd(self):
        src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": "$val"},
                                Flags="SubIFD", FixFormat="ifd")
        self.assertIn("fix_format: None, sub_ifd: true,", src)
        self.assertEqual(stats["ifd_subdir_edge_sub_ifd"], 1)
        # FixFormat alone (no Flags) still marks the SubIFD alternative.
        src, _ = self._edge(FixFormat="ifd")
        self.assertIn("sub_ifd: true", src)
        # A scalar FixFormat is carried as data (write-side only: WriteExif.pl:1760).
        src, _ = self._edge(Flags="SubIFD", FixFormat="int8u")
        self.assertIn("fix_format: Some(Fmt::Int8u), sub_ifd: true,", src)
        src, stats = self._edge(FixFormat="bogus")
        self.assertEqual(src, "None")
        self.assertEqual(stats["ifd_subdir_refused_fixformat"], 1)
        # The un-sugared key, and its presence-only pre-regen form.
        src, _ = self._edge(SubIFD="1")
        self.assertIn("sub_ifd: true", src)
        src, _ = self._edge(_extra_keys=["SubIFD", "WriteGroup"])
        self.assertIn("sub_ifd: true", src)
        src, _ = self._edge()
        self.assertIn("sub_ifd: false", src)

    def test_max_subdirs_dir_name_validate(self):
        sd = {"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": "$val",
              "MaxSubdirs": "10", "DirName": "SubIFD", "Validate": "$val =~ /^\\0/"}
        src, stats = self._edge(sd, Flags="SubIFD")
        self.assertIn(
            'max_subdirs: Some(10), dir_name: Some("SubIFD"), validate: true, '
            'validation: None, processor: IfdSubdirProcessor::Native, unwalked: None }',
            src,
        )
        # Emitted AND counted: the walk refuses it, the census still sees it.
        self.assertEqual(stats["ifd_subdir_edge_modeled"], 1)
        self.assertEqual(stats["ifd_subdir_refused_validate"], 1)
        self.assertNotIn("ifd_subdir_refused_validate", codegen.GATE_A_DISQUALIFYING)
        src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "MaxSubdirs": "lots"})
        self.assertEqual((src, stats["ifd_subdir_refused_unreadable"]), ("None", 1))

    def test_process_proc(self):
        pp = {"__perl": "CODE", "__name": "Image::ExifTool::ProcessBinaryData"}
        src, stats = self._edge({"TagTable": "Image::ExifTool::Minolta::CameraSettings", "ProcessProc": pp})
        self.assertIn('module: "Minolta", table: "CameraSettings"', src)
        self.assertEqual(stats["ifd_subdir_edge_target_binary"], 1)
        # Any other ProcessProc is Perl the walk cannot run: the edge is
        # emitted with the sub's name as its `unwalked` reason and counted
        # (NOT disqualifying); `descend` returns on it (ifd_engine.rs), the
        # `validate` precedent.
        for name in ("Image::ExifTool::ProcessTIFF", "Image::ExifTool::MakerNotes::ProcessUnknown"):
            pp = {"__perl": "CODE", "__name": name}
            src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "ProcessProc": pp})
            self.assertIn('module: "Olympus", table: "Equipment", ', src)
            self.assertTrue(
                src.endswith(
                    f'validate: false, validation: None, processor: IfdSubdirProcessor::Native, '
                    f'unwalked: Some("ProcessProc {name}") }})'
                ),
                src,
            )
            self.assertEqual(
                _plain(stats),
                {"ifd_subdir_processproc_unwalked": 1, "ifd_subdir_edge_modeled": 1,
                 "ifd_subdir_edge_target_ifd": 1},
                name,
            )
            self.assertEqual(stats["ifd_subdir_unwalked_reasons"][f"ProcessProc {name}"], 1)
        src, _ = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "ProcessProc": {"__perl": "CODE"}})
        self.assertIn('unwalked: Some("ProcessProc <unnamed CODE>")', src)
        self.assertNotIn("ifd_subdir_processproc_unwalked", codegen.GATE_A_DISQUALIFYING)
        self.assertNotIn("ifd_subdir_refused_processproc", codegen.GATE_A_DISQUALIFYING)

    def test_authenticated_serial_target_compiles_the_shared_u16_validation(self):
        # The edge's target is selected by its captured target-table processor;
        # no source module/table/tag spelling participates in this admission.
        from test_directory_validation import CALL, NAME
        from test_native_reader_contract import bound_helper, snapshot

        reader = snapshot()
        processor = {
            "__perl": "CODE", "resolved": True,
            "__name": "Image::ExifTool::Any::ProcessSerialData",
            "__deparse": "closed processor body",
            "source_file": "Image/ExifTool/Any.pm", "source_sha256": "4" * 64,
        }
        ctx = codegen.IfdGenContext(
            ifd_tables=set(), binary_tables=set(),
            serial_tables={("Any", "Child"): processor},
            table_processors={("Any", "Child"): processor},
            validation_helpers={NAME: bound_helper(reader)},
            reader_contracts={"unsigned16": reader},
        )
        stats = codegen.new_ifd_stats()
        src = codegen.compile_ifd_subdir(
            {"SubDirectory": {
                "TagTable": "Image::ExifTool::Any::Child", "Validate": CALL,
            }},
            stats,
            ctx,
        )
        self.assertIn("validation: Some(U16SizeCheck { offset: 0, expected: &[SizeExpectation::Relative(0)]", src)
        self.assertIn("processor: IfdSubdirProcessor::Serial", src)
        self.assertEqual(_plain(stats), {
            "ifd_subdir_edge_modeled": 1,
            "ifd_subdir_edge_target_serial": 1,
            "ifd_subdir_validate_compiled": 1,
        })

    def test_serial_override_or_helper_source_change_refuses_execution(self):
        from test_directory_validation import CALL, NAME
        from test_native_reader_contract import bound_helper, snapshot

        reader = snapshot()
        processor = {
            "__perl": "CODE", "resolved": True,
            "__name": "Image::ExifTool::Any::ProcessSerialData",
            "__deparse": "closed processor body",
            "source_file": "Image/ExifTool/Any.pm", "source_sha256": "4" * 64,
        }
        ctx = codegen.IfdGenContext(
            ifd_tables=set(), binary_tables=set(),
            serial_tables={("Any", "Child"): processor},
            table_processors={("Any", "Child"): processor},
            validation_helpers={NAME: bound_helper(reader)},
            reader_contracts={"unsigned16": reader},
        )
        def emit(subdir):
            stats = codegen.new_ifd_stats()
            return codegen.compile_ifd_subdir({"SubDirectory": subdir}, stats, ctx), stats

        changed = dict(processor, source_sha256="5" * 64)
        src, stats = emit({
            "TagTable": "Image::ExifTool::Any::Child", "ProcessProc": changed,
            "Validate": CALL,
        })
        self.assertIn("processor: IfdSubdirProcessor::Native", src)
        self.assertIn("validation: None", src)
        self.assertIn("ProcessProc override differs from target", src)
        self.assertEqual(stats["ifd_subdir_refused_validate"], 1)

        broken_helpers = dict(ctx.validation_helpers)
        broken = dict(broken_helpers[NAME])
        broken["__deparse"] = broken["__deparse"].replace("Get16u", "Get32u")
        ctx.validation_helpers = {NAME: broken}
        src, stats = emit({"TagTable": "Image::ExifTool::Any::Child", "Validate": CALL})
        self.assertIn("processor: IfdSubdirProcessor::Native", src)
        self.assertIn("validation: None", src)
        self.assertIn('unwalked: Some("serial Validate lacks authenticated primitive")', src)
        self.assertEqual(stats["ifd_subdir_refused_validate"], 1)

    def test_base_through_the_existing_grammar(self):
        src, _ = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment",
                             "Start": "$valuePtr + 8", "Base": "$start - 8"})
        self.assertIn("base: Some(&BaseExpr::Sub(&BaseExpr::Start, &BaseExpr::Const(8))),", src)
        src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "Base": "$val + 2"})
        self.assertEqual((src, stats["ifd_subdir_refused_base"]), ("None", 1))

    def test_tag_table_and_unmodeled_keys(self):
        src, stats = self._edge({"Start": "$val"})
        self.assertEqual((src, stats["ifd_subdir_refused_tagtable"]), ("None", 1))
        src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus", "Start": "$val"})
        self.assertEqual((src, stats["ifd_subdir_refused_tagtable"]), ("None", 1))
        for key in ("FixBase", "OffsetPt", "EntryBased", "RelativeBase", "Magic", "SomeFutureKey"):
            src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", key: "1"})
            self.assertEqual(src, "None", key)
            self.assertEqual(stats["ifd_subdir_refused_unmodeled_key"], 1, key)
            self.assertEqual(stats["ifd_subdir_unmodeled_keys"][key], 1, key)
        # WriteProc is write-side and ignored.
        src, _ = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "WriteProc": {"__perl": "CODE"}})
        self.assertIn("IfdSubdirEdge {", src)

    def test_target_kind_census(self):
        for table, kind in (("Olympus::Equipment", "ifd"), ("Minolta::CameraSettings", "binary"),
                            ("PrintIM::Main", "other")):
            _, stats = self._edge({"TagTable": f"Image::ExifTool::{table}"})
            self.assertEqual(stats[f"ifd_subdir_edge_target_{kind}"], 1, table)

    def test_makernotes_marked_edges_are_counted(self):
        _, stats = self._edge(_extra_keys=["MakerNotes"])
        self.assertEqual(stats["ifd_subdir_edge_makernotes"], 1)

    def test_tag_with_a_refused_edge_keeps_the_flag(self):
        tag = {"Name": "Equipment",
               "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": "16"}}
        src, reason, stats = _emit(tag)
        self.assertIsNone(reason)
        self.assertIn("subdirectory: true, print_conv: false }", src)
        self.assertIn("subdir: None }", src)
        self.assertEqual(stats["ifd_subdir_refused_start"], 1)
        self.assertEqual(stats["omitted_subdirectory"], 1)
        # And a modeled one carries both the flag and the edge.
        tag["SubDirectory"]["Start"] = "$valuePtr"
        src, _, _ = _emit(tag)
        self.assertIn("subdirectory: true, print_conv: false }", src)
        self.assertIn("subdir: Some(IfdSubdirEdge { module: \"Olympus\", table: \"Equipment\", "
                      "start: IfdStart::ValuePtr(0), base: None, byte_order: IfdByteOrder::Inherit, "
                      "fix_format: None, sub_ifd: false, max_subdirs: None, dir_name: None, "
                      "validate: false, validation: None, processor: IfdSubdirProcessor::Native, "
                      "unwalked: None }) }", src)


class VariantGroups(unittest.TestCase):
    def _group(self, alternatives, stats=None):
        stats = codegen.new_ifd_stats() if stats is None else stats
        src = codegen.compile_ifd_variant_group({"_variants": alternatives}, 0x2010, stats, None, _ctx(), {})
        return src, stats

    def test_compiles_through_conds_py(self):
        alts = [
            {"Name": "EquipmentIFD", "Condition": '$format eq "int32u"', "Flags": "SubIFD",
             "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": "$val"}},
            {"Name": "Equipment", "Unknown": "1",
             "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment", "ByteOrder": "Unknown"}},
        ]
        src, stats = self._group(alts)
        self.assertTrue(src.startswith("IfdVariantGroup { id: 0x2010, alternatives: &[(Cond::FormatEq { value: \"int32u\", negate: false }, IfdTag { id: 0x2010, name: \"EquipmentIFD\""), src)
        self.assertIn("(Cond::Always, IfdTag { id: 0x2010, name: \"Equipment\"", src)
        self.assertIn("unknown: true", src)
        self.assertEqual(stats["tag_variant_emitted"], 1)
        self.assertEqual(stats["ifd_variant_alternatives"], 2)
        self.assertEqual(stats["ifd_subdir_edge_modeled"], 2)

    def test_refused_atomically_on_a_condition_outside_the_grammar(self):
        alts = [
            {"Name": "Equipment", "Condition": "$$self{TIFF_TYPE} eq 'SRW' and $$self{PATH}[-2] eq 'IFD1'",
             "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment"}},
            {"Name": "EquipmentIFD", "Flags": "SubIFD",
             "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": "$val"}},
        ]
        src, stats = self._group(alts)
        self.assertIsNone(src)
        self.assertEqual(_plain(stats), {"tag_variant_cond_unsupported": 1})
        self.assertEqual(stats["ifd_variant_cond_texts"]["$$self{TIFF_TYPE} eq 'SRW' and $$self{PATH}[-2] eq 'IFD1'"], 1)
        # No partial credit leaked from the alternative that would have compiled.
        self.assertEqual(stats["ifd_subdir_edge_modeled"], 0)

    def test_refused_atomically_on_a_per_tag_reason(self):
        alts = [
            {"Name": "StripOffsets", "Condition": '$$self{TIFF_TYPE} eq "CR2"', "Flags": "IsOffset"},
            {"Name": "OtherImageStart", "Writable": "int32u"},
        ]
        src, stats = self._group(alts)
        self.assertIsNone(src)
        self.assertEqual(_plain(stats), {"tag_variant_field_unsupported": 1})
        self.assertEqual(stats["ifd_variant_field_reasons"]["ifd_isoffset_unsupported"], 1)

    def test_nested_variants_are_refused(self):
        src, stats = self._group([{"_variants": [{"Name": "X"}]}])
        self.assertIsNone(src)
        self.assertEqual(stats["tag_variant_cond_unsupported"], 1)


class TableLiteral(unittest.TestCase):
    def test_sorted_by_id_with_group_defaulting_and_set_group1_flag(self):
        tbl = {
            "meta": {"GROUPS": {"0": "MakerNotes", "2": "Camera"}, "SET_GROUP1": "1", "PRIORITY": "0"},
            "tags": {
                "515": {"Name": "BWMode", "Writable": "int16u"},
                "1": {"Name": "First"},
                "8208": {"_variants": [{"Name": "A", "Condition": '$format eq "ifd"'}, {"Name": "B"}]},
                "256": {"Name": "Thumb", "Flags": "Binary"},
            },
        }
        run = codegen.new_ifd_stats()
        src = codegen.gen_ifd_table("Olympus", "Main", tbl, run, None, _ctx())
        ids = re.findall(r"IfdTag \{ id: (0x[0-9a-f]{4})", src.split("variants:")[0])
        self.assertEqual(ids, ["0x0001", "0x0100", "0x0203"])
        self.assertIn("IfdVariantGroup { id: 0x2010,", src)
        self.assertIn('group0: "MakerNotes",\n    group1: "Olympus",\n    group2: "Camera",', src)
        # The declared payload verbatim (a flag ExifTool tests for truth,
        # Exif.pm:7183); the walk reads `is_some()`.
        self.assertIn('set_group1: Some("1"),\n    priority: Some(0),', src)
        self.assertIn("gate_a: GateA { blocked_by: &[] }", src)
        self.assertIn("pub static IFD_OLYMPUS_MAIN: IfdTable = IfdTable {", src)
        self.assertIn("SET_GROUP1` declared (Exif.pm:7183", src)
        self.assertEqual(run["ifd_set_group1_flag"], 1)
        self.assertEqual(run["ifd_gate_a_pass"], 1)
        self.assertEqual(run["ifd_tag_emitted"], 3)
        self.assertEqual(run["tag_variant_emitted"], 1)

    def test_empty_table_is_still_emitted(self):
        tbl = {"meta": {}, "tags": {"Duration": {"Name": "Duration"}}}
        run = codegen.new_ifd_stats()
        src = codegen.gen_ifd_table("APE", "Composite", tbl, run, None, _ctx())
        self.assertIn("tags: &[],\n    variants: &[],", src)
        self.assertIn('blocked_by: &[("ifd_tag_id_unrepresentable", 1)]', src)
        self.assertEqual(run["ifd_table_emitted"], 1)

    def test_identifier_rule(self):
        self.assertEqual(codegen.ifd_table_ident("Olympus", "Main"), "IFD_OLYMPUS_MAIN")
        self.assertEqual(codegen.ifd_table_ident("ICC_Profile", "Header"), "IFD_ICC_PROFILE_HEADER")

    def test_every_ifd_counter_is_reported(self):
        # The `missed` assertion in print_ifd_report is the runtime guard; this
        # pins the static half so a new counter cannot be added to
        # GATE_A_DISQUALIFYING without a REPORT line.
        reported = {key for _, rows in codegen.IFD_REPORT for _, key in rows}
        for key in codegen.GATE_A_DISQUALIFYING:
            if key.startswith("ifd_"):
                self.assertIn(key, reported, key)


class SliceIfd1GateA(unittest.TestCase):
    """The Gate A policy change that lets `Exif::Main` be walked at a NAMED
    directory (docs/superpowers/specs/2026-09-06-ifd-tables-design.md v1.1):
    `Format => 'binary'` is the unsized undef (G1); a SubDirectory with no
    `TagTable` or a ProcessProc other than ProcessBinaryData is emitted
    unwalked (G2); a `_variants` group that could never give the walk
    anything to get right is classified unreported and emits nothing (G3).
    Every affected id is absent and counted; nothing here disqualifies."""

    def test_binary_is_the_unsized_undef(self):
        # ExifTool.pm:6236 `binary => 1` in %formatSize; ReadValue :6308-6309
        # reads undef/binary/string with one substr and NUL-truncates only
        # string (:6311). ColorMap: Exif.pm:961-965.
        src, reason, stats = _emit({"Name": "ColorMap", "Format": "binary", "Binary": "1"}, tag_id=0x0140)
        self.assertIsNone(reason)
        self.assertIn('name: "ColorMap", format: Some(Fmt::Undef(0)), count: None', src)
        self.assertIn("binary: true", src)
        self.assertEqual(stats["ifd_format_unsized"], 1)
        self.assertEqual(stats["ifd_format_binary_as_undef"], 1)
        self.assertEqual(stats["ifd_format_unsupported"], 0)
        # The domain follows undef (bytes): the same refusal class as a bare
        # undef under a numeric PrintConv, never a wrong number.
        _, _, a = _emit({"Name": "A", "Format": "binary", "PrintConv": {"0": "x"}})
        _, _, b = _emit({"Name": "B", "Format": "undef", "PrintConv": {"0": "x"}})
        self.assertEqual(
            {k: v for k, v in _plain(a).items() if k != "ifd_format_binary_as_undef"}, _plain(b)
        )
        # A sized `binary[N]` is not a spelling ExifTool has; still refused.
        _, reason, stats = _emit({"Name": "C", "Format": "binary[4]"})
        self.assertEqual((reason, stats["ifd_format_unsupported"]), ("ifd_format_unsupported", 1))

    def _same_table(self, sd, enclosing=("Exif", "Main"), **extra):
        tag = {"Name": "ExifOffset", "SubDirectory": sd}
        tag.update(extra)
        stats = codegen.new_ifd_stats()
        src = codegen.compile_ifd_subdir(codegen.expand_flags(tag, stats), stats, _ctx(), enclosing)
        return src, stats

    def test_no_tag_table_is_the_enclosing_table_emitted_unwalked(self):
        # Exif.pm:6939-6944: `$newTagTable = $tagTablePtr; # use existing table`.
        src, stats = self._same_table({"Start": "$val", "DirName": "ExifIFD"}, SubIFD="1")
        self.assertEqual(
            src,
            'Some(IfdSubdirEdge { module: "Exif", table: "Main", start: IfdStart::Val(0), base: None, '
            "byte_order: IfdByteOrder::Inherit, fix_format: None, sub_ifd: true, max_subdirs: None, "
            'dir_name: Some("ExifIFD"), validate: false, validation: None, '
            'processor: IfdSubdirProcessor::Native, '
            'unwalked: Some("same-table recursion (TagTable absent)") })',
        )
        self.assertEqual(
            _plain(stats),
            {"ifd_subdir_same_table_unwalked": 1, "ifd_subdir_edge_modeled": 1,
             "ifd_subdir_edge_target_other": 1, "ifd_subdir_edge_sub_ifd": 1},
        )
        self.assertEqual(stats["ifd_subdir_unwalked_reasons"]["same-table recursion (TagTable absent)"], 1)
        self.assertNotIn("ifd_subdir_same_table_unwalked", codegen.GATE_A_DISQUALIFYING)
        # With the enclosing table unknown there is no honest name: refused as before.
        src, stats = self._same_table({"Start": "$val"}, enclosing=None)
        self.assertEqual((src, _plain(stats)), ("None", {"ifd_subdir_refused_tagtable": 1}))
        # A TagTable that is present but unparseable is still a refusal.
        src, stats = self._same_table({"TagTable": "Image::ExifTool::Olympus", "Start": "$val"})
        self.assertEqual((src, _plain(stats)), ("None", {"ifd_subdir_refused_tagtable": 1}))

    def test_profile_ifd_shape_names_every_reason(self):
        # Exif::Main 0xc6f5 ProfileIFD: no TagTable, ProcessTiffIFD, and a
        # `Magic` key the schema does not model -- named, not refused,
        # because nothing reads an unwalked edge's keys.
        sd = {"Start": "$val", "Base": "$start", "Magic": "17234", "MaxSubdirs": "10", "DirName": "ProfileIFD",
              "ProcessProc": {"__perl": "CODE", "__name": "Image::ExifTool::Exif::ProcessTiffIFD"}}
        src, stats = self._same_table(dict(sd), Flags="SubIFD")
        self.assertIn(
            'unwalked: Some("same-table recursion (TagTable absent); '
            'ProcessProc Image::ExifTool::Exif::ProcessTiffIFD; unmodeled SubDirectory key(s) Magic")',
            src,
        )
        self.assertIn('base: Some(&BaseExpr::Start), byte_order: IfdByteOrder::Inherit, fix_format: None, '
                      'sub_ifd: true, max_subdirs: Some(10), dir_name: Some("ProfileIFD")', src)
        self.assertEqual(stats["ifd_subdir_refused_unmodeled_key"], 0)
        self.assertEqual(stats["ifd_subdir_same_table_unwalked"], 1)
        self.assertEqual(stats["ifd_subdir_processproc_unwalked"], 1)
        # An unmodeled key on a WALKABLE edge still refuses (the existing rule).
        src, stats = self._same_table({"TagTable": "Image::ExifTool::Olympus::Equipment", "Magic": "1"})
        self.assertEqual((src, stats["ifd_subdir_refused_unmodeled_key"]), ("None", 1))
        # Every other field must still compile: an unwalked edge with a Start
        # outside the grammar is refused with exactly that one counter.
        sd["Start"] = "4"
        src, stats = self._same_table(sd, Flags="SubIFD")
        self.assertEqual((src, _plain(stats)), ("None", {"ifd_subdir_refused_start": 1}))

    def _group(self, alternatives, ctx=None, tag_id=0x0111, enclosing=("Exif", "Main")):
        stats = codegen.new_ifd_stats()
        src = codegen.compile_ifd_variant_group(
            {"_variants": alternatives}, tag_id, stats, None, ctx or _ctx(), {}, enclosing=enclosing
        )
        return src, stats

    def test_offset_only_group_is_unreported_before_any_condition_compiles(self):
        # Exif::Main 0x0111 (Exif.pm:601): every alternative IsOffset/OffsetPair,
        # Conditions outside the grammar. Nothing emitted, nothing partial.
        alts = [
            {"Name": "StripOffsets", "Condition": "$$self{TIFF_TYPE} eq 'CR2' and $$self{DIR_NAME} eq 'IFD0'",
             "Flags": ["IsOffset", "OffsetPair"]},
            {"Name": "PreviewImageStart", "Condition": '$$self{DIR_NAME} eq "IFD1"',
             "_extra_keys": ["DataTag", "IsOffset", "OffsetPair"]},
        ]
        src, stats = self._group(alts)
        self.assertIs(src, codegen.IFD_VARIANT_UNREPORTED)
        self.assertEqual(_plain(stats), {"ifd_variant_unreported_skipped": 1})
        self.assertEqual(stats["ifd_variant_cond_texts"], Counter())
        self.assertEqual(
            stats["ifd_variant_unreported_ids"]["Exif::Main 0x0111 (2 offset-class/unwalkable-SubDirectory alternatives)"], 1
        )
        self.assertNotIn("ifd_variant_unreported_skipped", codegen.GATE_A_DISQUALIFYING)

    def test_unwalkable_subdirectory_alternatives_are_unreported(self):
        # Exif::Main 0x014a (Exif.pm:1006-1040): SubIFD with no TagTable, then
        # A100DataOffset (IsOffset).
        alts = [
            {"Name": "SubIFD", "Condition": '$$self{TIFF_TYPE} ne "ARW"', "Flags": "SubIFD",
             "SubDirectory": {"Start": "$val", "MaxSubdirs": "10"}},
            {"Name": "A100DataOffset", "_extra_keys": ["IsOffset"]},
        ]
        src, stats = self._group(alts, tag_id=0x014a)
        self.assertIs(src, codegen.IFD_VARIANT_UNREPORTED)
        self.assertEqual(_plain(stats), {"ifd_variant_unreported_skipped": 1})
        # ProcessProc other than ProcessBinaryData is the other unwalkable shape.
        alts = [{"Name": "X", "Condition": "$$self{Foo}",
                 "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment",
                                  "ProcessProc": {"__perl": "CODE", "__name": "Image::ExifTool::ProcessTIFF"}}}]
        src, stats = self._group(alts)
        self.assertIs(src, codegen.IFD_VARIANT_UNREPORTED)
        self.assertEqual(_plain(stats), {"ifd_variant_unreported_skipped": 1})
        # An empty group is not "every alternative": it compiles as before.
        src, stats = self._group([])
        self.assertEqual(src, "IfdVariantGroup { id: 0x0111, alternatives: &[] }")

    def test_one_reported_alternative_keeps_the_atomic_refusal(self):
        # A plain tag is reported: the group takes the compiling path and is
        # refused whole on the first uncompilable Condition, no partial credit.
        alts = [
            {"Name": "StripOffsets", "Condition": "$$self{TIFF_TYPE} eq 'CR2'", "Flags": "IsOffset"},
            {"Name": "Plain", "Writable": "int32u"},
        ]
        src, stats = self._group(alts)
        self.assertIsNone(src)
        self.assertEqual(_plain(stats), {"tag_variant_cond_unsupported": 1})
        # So is a SubDirectory with a WALKABLE edge (Olympus::Main 0x2010,
        # Canon::Main 0x000d): its target's tags are the value.
        alts = [
            {"Name": "Equipment", "Condition": "$$self{TIFF_TYPE} eq 'SRW'",
             "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment"}},
            {"Name": "SubIFD", "Flags": "SubIFD", "SubDirectory": {"Start": "$val"}},
        ]
        src, stats = self._group(alts)
        self.assertIsNone(src)
        self.assertEqual(_plain(stats), {"tag_variant_cond_unsupported": 1})
        # And the compiling path is unchanged when the Conditions compile.
        alts[0]["Condition"] = '$format eq "int32u"'
        src, stats = self._group(alts, enclosing=("Olympus", "Main"))
        self.assertTrue(src.startswith("IfdVariantGroup { id: 0x0111, alternatives: &[(Cond::FormatEq"), src)
        self.assertIn('module: "Olympus", table: "Main", start: IfdStart::Val(0)', src)
        self.assertIn('unwalked: Some("same-table recursion (TagTable absent)")', src)
        self.assertEqual(stats["tag_variant_emitted"], 1)
        self.assertEqual(stats["ifd_subdir_same_table_unwalked"], 1)

    def test_makernotes_dispatch_is_recognised_by_identity(self):
        # Exif.pm:2496 `0x927c => \@Image::ExifTool::MakerNotes::Main`. The
        # generic rule cannot cover it: MakerNoteMinolta3/Samsung1a/UnknownText/
        # UnknownBinary are plain Binary values with no SubDirectory. The dump
        # inlines the array as `_variants` and dumps it under
        # `modules.MakerNotes.arrays.Main.rows`; equality of the two lists is
        # the fact (verified on the 13.59 dump: 94 == 94 rows).
        rows = [
            {"Name": "MakerNoteApple", "Condition": "$$valPt =~ /^Apple iOS/", "Binary": "1", "Format": "undef",
             "SubDirectory": {"TagTable": "Image::ExifTool::Apple::Main"}},
            {"Name": "MakerNoteMinolta3", "Condition": "$$valPt =~ /^\\0\\x1b/", "Binary": "1", "Format": "undef"},
            {"Name": "MakerNoteUnknownBinary", "Binary": "1", "Format": "undef"},
        ]
        doc = {"modules": {"MakerNotes": {"tables": {}, "arrays": {"Main": {
            "full_name": "Image::ExifTool::MakerNotes::Main", "row_count": 3, "rows": rows}}}}}
        ctx = codegen.IfdGenContext.from_doc(doc)
        self.assertEqual(ctx.makernotes_main, rows)
        src, stats = self._group([dict(r) for r in rows], ctx=ctx, tag_id=0x927c)
        self.assertIs(src, codegen.IFD_VARIANT_UNREPORTED)
        self.assertEqual(_plain(stats), {"ifd_variant_makernotes_dispatch": 1})
        self.assertEqual(stats["ifd_variant_unreported_ids"]["Exif::Main 0x927c MakerNotes::Main dispatch (3 alternatives)"], 1)
        self.assertNotIn("ifd_variant_makernotes_dispatch", codegen.GATE_A_DISQUALIFYING)
        # Not the array (a subset), or a dump without it: the compiling path,
        # where the plain-value alternative makes the group reportable.
        src, stats = self._group([dict(r) for r in rows[1:]], ctx=ctx, tag_id=0x927c)
        self.assertIsNot(src, codegen.IFD_VARIANT_UNREPORTED)
        self.assertEqual(stats["ifd_variant_makernotes_dispatch"], 0)
        self.assertFalse(codegen.IfdGenContext.from_doc({"modules": {}}).is_makernotes_dispatch(rows))
        src, stats = self._group([dict(r) for r in rows], tag_id=0x927c)
        self.assertIsNot(src, codegen.IFD_VARIANT_UNREPORTED)

    def test_table_with_unreported_group_and_unwalked_edge_passes_gate_a(self):
        tbl = {"meta": {}, "tags": {
            "273": {"_variants": [
                {"Name": "StripOffsets", "Condition": "$$self{TIFF_TYPE} eq 'CR2'", "Flags": "IsOffset"},
                {"Name": "PreviewImageStart", "_extra_keys": ["IsOffset", "OffsetPair"]},
            ]},
            "34665": {"Name": "ExifOffset", "SubDirectory": {"DirName": "ExifIFD", "Start": "$val"}, "SubIFD": "1"},
            "320": {"Name": "ColorMap", "Format": "binary", "Binary": "1"},
        }}
        run = codegen.new_ifd_stats()
        src = codegen.gen_ifd_table("Exif", "Main", tbl, run, None, _ctx())
        self.assertIn("gate_a: GateA { blocked_by: &[] }", src)
        self.assertIn("variants: &[]", src)
        self.assertNotIn("0x0111", src)
        self.assertIn('module: "Exif", table: "Main", start: IfdStart::Val(0)', src)
        self.assertIn('unwalked: Some("same-table recursion (TagTable absent)")', src)
        self.assertIn('name: "ColorMap", format: Some(Fmt::Undef(0))', src)
        self.assertEqual(run["tag_variant_skipped"], 0)
        self.assertEqual(run["ifd_variant_unreported_skipped"], 1)
        self.assertEqual(run["ifd_subdir_same_table_unwalked"], 1)
        self.assertEqual(run["ifd_tag_emitted"], 2)
        self.assertEqual(run["ifd_gate_a_pass"], 1)
        # And the REPORT accounts for every counter this path touches.
        reported = {key for _, rows in codegen.IFD_REPORT for _, key in rows}
        touched = {k for k, v in run.items() if not isinstance(v, Counter)}
        self.assertEqual(touched - reported, set())


def _find_dump():
    env = os.environ.get("OXIDEX_TABLES_JSON")
    if env:
        return Path(env), "OXIDEX_TABLES_JSON"
    pin = (ROOT / ".exiftool-version").read_text().strip()
    cached = ROOT / "target" / "exiftool-src" / f"tables-{pin}.json"
    if cached.is_file():
        return cached, "regen.sh cache"
    return None, f"neither $OXIDEX_TABLES_JSON nor {cached} exists"


class WholeDump(unittest.TestCase):
    """The census over a real `dump_tables.pl` JSON (the i7's Perl 5.38.2 dump
    of 13.59 in practice). Structural assertions only need the dump; the
    conversion counts additionally need the committed ledger to validate
    against it, which `test_committed_ledger_validates_against_the_dump`
    reports as a skip (with the digest message) rather than silently
    downgrading to an unledgered run."""

    @classmethod
    def setUpClass(cls):
        path, source = _find_dump()
        if path is None:
            raise unittest.SkipTest(f"no tables dump to census: {source}")
        cls.dump_path = path
        with open(path, encoding="utf-8") as fh:
            cls.doc = json.load(fh)
        version = str(cls.doc.get("exiftool_version") or "")
        ledger = ROOT / "tools" / "exiftool-tables" / "expr_oracle_ledger.json"
        cls.ledger_error = None
        try:
            cls.verified = codegen.load_oracle_ledger(str(ledger), str(path), version)
        except SystemExit as exc:
            cls.ledger_error = str(exc)
            cls.verified = None
        names = sorted(cls.doc["modules"])
        cls.chunks, cls.index_rows, cls.stats, cls.identity_ledger = codegen.gen_ifd_tables(cls.doc, names, cls.verified)

    def _selected(self):
        return [
            (m, t)
            for m, mod in sorted(self.doc["modules"].items())
            for t, tbl in sorted(mod["tables"].items())
            if codegen.is_ifd_table(tbl.get("meta") or {})
        ]

    def test_committed_ledger_validates_against_the_dump(self):
        if self.ledger_error:
            self.skipTest(self.ledger_error)
        self.assertEqual(self.stats["expr_refused_oracle"], 0, "the ledger census is dump-wide")

    def test_every_selected_table_is_emitted_exactly_once(self):
        selected = self._selected()
        self.assertGreater(len(selected), 450)
        self.assertLess(len(selected), 550)
        self.assertEqual(len(self.index_rows), len(selected))
        self.assertEqual(self.stats["ifd_table_emitted"], len(selected))
        self.assertEqual(
            [f"    &{codegen.ifd_table_ident(m, t)}," for m, t in selected], self.index_rows,
            "ALL_IFD_TABLES must list every selected table, sorted by (module, table)",
        )
        committed = ROOT / "src" / "exiftool_tables" / "ifd" / "mod.rs"
        if committed.is_file():
            statics = re.findall(r"^pub static (IFD_\w+): IfdTable", table_modules.read_logical(committed), re.M)
            self.assertEqual(len(statics), len(selected), "committed ifd/ table count")

    def test_tags_and_variants_are_sorted_and_unique_per_table(self):
        for chunk in self.chunks:
            name = re.search(r"pub static (IFD_\w+)", chunk).group(1)
            tags_part, _, variants_part = chunk.partition("\n    variants:")
            ids = [int(x, 16) for x in re.findall(r"\n    IfdTag \{ id: (0x[0-9a-f]{4})", tags_part)]
            self.assertEqual(ids, sorted(set(ids)), f"{name} tags")
            vids = [int(x, 16) for x in re.findall(r"IfdVariantGroup \{ id: (0x[0-9a-f]{4})", variants_part)]
            self.assertEqual(vids, sorted(set(vids)), f"{name} variants")
            self.assertFalse(set(ids) & set(vids), f"{name} has an id in both tags and variants")

    def test_olympus_main_bwmode(self):
        olympus = next(c for c in self.chunks if "pub static IFD_OLYMPUS_MAIN:" in c)
        self.assertIn(
            'IfdTag { id: 0x0203, name: "BWMode", format: None, count: None, '
            'writable: Some("int16u"), groups: TagGroups::NONE, flags: IfdFlags::NONE, '
            "condition: None, omitted: Omitted::NONE, raw_conv: None, value_conv: None, "
            'print_conv: PrintConv::IntEnum(&[(0, "Off"), (1, "On"), (6, "(none)")]), subdir: None }',
            olympus,
        )
        self.assertIn('group0: "MakerNotes",\n    group1: "Olympus",\n    group2: "Camera",', olympus)

    def test_the_census_is_accounted_for(self):
        # Every counter the pass touched has a REPORT line (print_ifd_report
        # would abort otherwise); check it without printing 100 lines.
        reported = {key for _, rows in codegen.IFD_REPORT for _, key in rows}
        touched = {k for k, v in self.stats.items() if not isinstance(v, Counter)}
        self.assertEqual(touched - reported, set())
        self.assertGreater(self.stats["ifd_tag_emitted"], 5000)
        self.assertGreater(self.stats["ifd_flag_unknown"], 100, "Unknown is a flag, never a drop")
        self.assertGreater(self.stats["ifd_subdir_edge_modeled"], 300)
        self.assertEqual(self.stats["ifd_set_group1_flag"], 6, "Exif::Main, Kodak x2, Sony x2, SonyIDC")

    def test_exif_main_passes_gate_a_with_every_unreported_id_absent(self):
        # Slice IFD1 (spec section 2): Exif::Main's five Gate A counters clear
        # without a single approximation -- the affected ids are absent and
        # counted, the five refused edges are emitted unwalked.
        exif = next(c for c in self.chunks if "pub static IFD_EXIF_MAIN:" in c)
        self.assertIn("gate_a: GateA { blocked_by: &[] }", exif)
        for tid in ("0x0111", "0x0117", "0x014a", "0x0201", "0x0202", "0x927c"):
            self.assertNotIn(f"id: {tid},", exif, f"{tid} must be absent (offset class / dispatch)")
        self.assertIn('id: 0x0140, name: "ColorMap", format: Some(Fmt::Undef(0))', exif)
        self.assertIn('id: 0x8649, name: "PhotoshopSettings", format: Some(Fmt::Undef(0))', exif)
        self.assertEqual(exif.count('unwalked: Some("same-table recursion (TagTable absent)'), 4)
        self.assertEqual(exif.count('unwalked: Some("ProcessProc Image::ExifTool::ProcessSubTIFF")'), 1)
        self.assertEqual(self.stats["ifd_variant_makernotes_dispatch"], 1, "0x927c and nothing else")
        self.assertEqual(self.stats["ifd_subdir_same_table_unwalked"], 4, "all four in Exif::Main")
        self.assertEqual(self.stats["ifd_format_binary_as_undef"], 2, "ColorMap, PhotoshopSettings")


if __name__ == "__main__":
    unittest.main()
