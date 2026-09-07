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
import unittest
from collections import Counter
from pathlib import Path

import codegen
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
            "omitted: Omitted::NONE, raw_conv: None, value_conv: None, "
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
        # A str-domain tag must not run a num-domain expression.
        tag = {"Name": "T", "Writable": "string", "PrintConv": {"kind": "expr", "expr": "$val / 10"}}
        src, _, stats = _emit(tag, verified=verified)
        self.assertIn("print_conv: PrintConv::None", src)
        self.assertEqual(stats["expr_refused_input_domain"], 1)
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

    def test_condition_on_a_single_tag_is_omitted_unless_resolved(self):
        tag = {"Name": "T", "Condition": '$$self{Model} =~ /E-M1/'}
        src, _, stats = _emit(tag)
        self.assertIn("condition: true", src)
        src, _, _ = _emit(tag, condition_resolved=True)
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
        self.assertIn('max_subdirs: Some(10), dir_name: Some("SubIFD"), validate: true }', src)
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
        for name in ("Image::ExifTool::ProcessTIFF", "Image::ExifTool::MakerNotes::ProcessUnknown"):
            pp = {"__perl": "CODE", "__name": name}
            src, stats = self._edge({"TagTable": "Image::ExifTool::Olympus::Equipment", "ProcessProc": pp})
            self.assertEqual(src, "None", name)
            self.assertEqual(_plain(stats), {"ifd_subdir_refused_processproc": 1}, name)

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
                      "validate: false }) }", src)


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
        self.assertTrue(src.startswith("IfdVariantGroup { id: 0x2010, alternatives: &[(Cond::FormatEq { value: \"int32u\" }, IfdTag { id: 0x2010, name: \"EquipmentIFD\""), src)
        self.assertIn("(Cond::Always, IfdTag { id: 0x2010, name: \"Equipment\"", src)
        self.assertIn("unknown: true", src)
        self.assertEqual(stats["tag_variant_emitted"], 1)
        self.assertEqual(stats["ifd_variant_alternatives"], 2)
        self.assertEqual(stats["ifd_subdir_edge_modeled"], 2)

    def test_refused_atomically_on_a_condition_outside_the_grammar(self):
        alts = [
            {"Name": "Equipment", "Condition": '$format ne "ifd" and $format ne "int32u"',
             "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment"}},
            {"Name": "EquipmentIFD", "Flags": "SubIFD",
             "SubDirectory": {"TagTable": "Image::ExifTool::Olympus::Equipment", "Start": "$val"}},
        ]
        src, stats = self._group(alts)
        self.assertIsNone(src)
        self.assertEqual(_plain(stats), {"tag_variant_cond_unsupported": 1})
        self.assertEqual(stats["ifd_variant_cond_texts"]['$format ne "ifd" and $format ne "int32u"'], 1)
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
        cls.chunks, cls.index_rows, cls.stats = codegen.gen_ifd_tables(cls.doc, names, cls.verified)

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
        committed = ROOT / "src" / "exiftool_tables" / "ifd_tables.rs"
        if committed.is_file():
            statics = re.findall(r"^pub static (IFD_\w+): IfdTable", committed.read_text(), re.M)
            self.assertEqual(len(statics), len(selected), "committed ifd_tables.rs table count")

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
            "omitted: Omitted::NONE, raw_conv: None, value_conv: None, "
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


if __name__ == "__main__":
    unittest.main()
